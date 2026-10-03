//! Turning a pack on disk into a playable [`SoundBank`].

use super::decode::{self, LEADING_SILENCE_WARN_SECONDS, MAX_SAMPLE_SECONDS};
use super::manifest::{self, Manifest};
use super::source::{MAX_AUDIO_FILE_BYTES, PackSource};
use super::{PackError, PackInfo, PackOrigin, Problem};
use crate::audio::{SoundBank, SoundMap, Variation};
use crate::input::KeyAction;
use crate::key::Key;
use std::cmp::Reverse;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::num::NonZeroUsize;
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError, mpsc};
use std::thread;

/// Distinct audio files one pack may reference (sound sets and preview together).
pub const MAX_AUDIO_FILES: usize = 2_000;
/// Total decoded audio per pack, counted in samples at [`LIMIT_REFERENCE_RATE`] whatever the
/// output rate (about 349 s of audio; 64 MB as f32 at 48 kHz). Counting at a fixed rate makes
/// the verdict independent of the user's audio device: a pack that validates at 48 kHz loads
/// on a 192 kHz interface too (using four times the memory there).
pub const MAX_TOTAL_SAMPLES: usize = 16 * 1024 * 1024;
/// The rate [`MAX_TOTAL_SAMPLES`] is measured at.
pub const LIMIT_REFERENCE_RATE: u32 = 48_000;

/// Upper bound on decode threads; more only adds memory (each holds a file in flight).
const MAX_DECODE_THREADS: usize = 8;
/// Below this many files per thread, starting threads (each builds its own resampler
/// tables) costs more than it saves.
const MIN_FILES_PER_THREAD: usize = 4;

pub struct LoadedPack {
    pub info: PackInfo,
    pub bank: SoundBank,
    /// Decoded preview clip at the same rate as the bank (for "click to hear"), with the
    /// pack's `volume` already applied: it plays outside the bank, so it carries the pack-level
    /// gain itself and sounds as loud as typing in the pack.
    pub preview: Option<Box<[f32]>>,
    /// Non-fatal problems (unknown fields, no release sounds, personal license, …).
    pub warnings: Vec<Problem>,
}

/// Opens a pack and parses its `pack.json`, without validating it or touching any audio.
/// For tools that need the manifest itself (per-group statistics, converters).
pub fn read_manifest(path: &Path) -> Result<Manifest, PackError> {
    open(path).map(|(_, manifest)| manifest)
}

/// Opens, parses and validates a pack and checks that every referenced file exists, without
/// decoding audio. Fast enough to run over every pack when listing them.
pub fn inspect(
    path: &Path,
    origin: PackOrigin,
    strict: bool,
) -> Result<(PackInfo, Vec<Problem>), PackError> {
    let stage = stage1(path, strict)?;
    if stage.has_errors() {
        return Err(PackError::new(path, stage.problems));
    }
    Ok((pack_info(&stage.manifest, path, origin), stage.problems))
}

/// Full load: inspect, then decode every referenced file, trim, resample to `sample_rate`,
/// enforce the size limits, and build the key → sample table via `manifest::resolve`.
/// Each distinct file is decoded once even if referenced many times.
///
/// `bank.samples` holds the distinct files that keys play, in path order. A preview file that
/// no key plays is decoded into `preview` only. All problems are reported together: every
/// file that fails to decode, is too long or pushes the pack over the size limit. A pack whose
/// pack.json has errors is rejected without decoding anything (see [`check_all`]).
pub fn load(path: &Path, origin: PackOrigin, sample_rate: u32) -> Result<LoadedPack, PackError> {
    load_pack(path, origin, sample_rate, Options::PLAYBACK)
}

/// What `taktak-pack validate` runs: [`load`] with the given strictness, reporting every
/// problem in one pass. When pack.json or its file references have errors, every referenced
/// file that exists is still decoded, so undecodable or overlong audio is reported in the same
/// run instead of only once pack.json is clean.
pub fn check_all(path: &Path, strict: bool, sample_rate: u32) -> Result<LoadedPack, PackError> {
    let options = Options { strict, decode_despite_errors: true, ..Options::PLAYBACK };
    load_pack(path, PackOrigin::User, sample_rate, options)
}

/// How [`load_pack`] treats a pack.
#[derive(Clone, Copy)]
struct Options {
    strict: bool,
    /// Decode the files that exist even when pack.json has errors (validation). Loading for
    /// playback only needs a verdict, so it stops at the first stage instead.
    decode_despite_errors: bool,
    /// Measured at [`LIMIT_REFERENCE_RATE`]; configurable so tests can reach it cheaply.
    max_total_samples: usize,
}

impl Options {
    const PLAYBACK: Options = Options {
        strict: false,
        decode_despite_errors: false,
        max_total_samples: MAX_TOTAL_SAMPLES,
    };
}

/// The first stage of every check: the opened pack, its manifest, the problems `validate` and
/// the file references produced (errors first), and the referenced files that do not exist.
/// `missing` is `None` when the files were not checked at all (a newer format, or too many).
struct Stage1 {
    source: PackSource,
    manifest: Manifest,
    problems: Vec<Problem>,
    missing: Option<HashSet<String>>,
}

impl Stage1 {
    fn has_errors(&self) -> bool {
        self.problems.iter().any(Problem::is_error)
    }
}

fn open(path: &Path) -> Result<(PackSource, Manifest), PackError> {
    let mut source = PackSource::open(path).map_err(|p| PackError::single(path, p))?;
    let bytes = source
        .read("pack.json", manifest::MAX_MANIFEST_BYTES as u64)
        .map_err(|p| PackError::single(path, p))?;
    let manifest = manifest::parse(&bytes).map_err(|problems| PackError::new(path, problems))?;
    Ok((source, manifest))
}

fn stage1(path: &Path, strict: bool) -> Result<Stage1, PackError> {
    let (mut source, manifest) = open(path)?;
    let mut problems = manifest::validate(&manifest, strict);
    // A newer format may lay files out differently; "needs a newer TakTak" says it all.
    let missing = (manifest.format == manifest::FORMAT_VERSION)
        .then(|| check_files(&mut source, &manifest, &mut problems))
        .flatten();
    problems.sort_by_key(|p| Reverse(p.severity));
    Ok(Stage1 { source, manifest, problems, missing })
}

fn pack_info(m: &Manifest, path: &Path, origin: PackOrigin) -> PackInfo {
    let optional = |text: &Option<String>| {
        text.as_deref().map(str::trim).filter(|t| !t.is_empty()).map(str::to_owned)
    };
    PackInfo {
        id: m.id.clone(),
        name: m.name.trim().to_owned(),
        version: optional(&m.version),
        author: m.author.trim().to_owned(),
        license: m.license.clone(),
        description: optional(&m.description),
        source: optional(&m.source),
        attribution: optional(&m.attribution),
        location: path.to_path_buf(),
        origin,
    }
}

/// Every file reference with its location (`"groups.space.press[1]"`), in the spec's field
/// order with groups and keys by name, like `manifest::validate`.
fn references(m: &Manifest) -> Vec<(String, &str)> {
    let mut out = Vec::new();
    if let Some(preview) = &m.preview {
        out.push(("preview".to_owned(), preview.as_str()));
    }
    for (section, sets) in [("groups", &m.groups), ("keys", &m.keys)] {
        for (name, set) in sets {
            for (field, files) in [("press", &set.press), ("release", &set.release)] {
                for (i, file) in files.iter().enumerate() {
                    out.push((format!("{section}.{name}.{field}[{i}]"), file.as_str()));
                }
            }
        }
    }
    out
}

/// The file-count limit, then existence of every referenced file, reported at every place
/// that references a missing one. Returns the missing files (with valid syntax), or `None`
/// when the count limit stopped the check.
fn check_files(
    source: &mut PackSource,
    m: &Manifest,
    out: &mut Vec<Problem>,
) -> Option<HashSet<String>> {
    let distinct = manifest::referenced_files(m).len();
    if distinct > MAX_AUDIO_FILES {
        // Checking thousands of files would only bury this under noise.
        out.push(Problem::error(
            "pack",
            format!(
                "pack references {distinct} distinct audio files; the limit is {MAX_AUDIO_FILES}"
            ),
        ));
        return None;
    }
    let mut missing: HashMap<&str, Option<String>> = HashMap::new();
    for (location, path) in references(m) {
        // Bad syntax is already reported by `validate`, at this same location.
        if manifest::check_path(path).is_err() {
            continue;
        }
        let why = missing
            .entry(path)
            .or_insert_with(|| (!source.exists(path)).then(|| missing_reason(source, path)));
        if let Some(why) = why {
            out.push(Problem::error(location, format!("{path:?}: {why}")));
        }
    }
    Some(missing.into_iter().filter(|(_, why)| why.is_some()).map(|(p, _)| p.to_owned()).collect())
}

/// The source's explanation for a file that does not exist, including its letter-case hint
/// ("names are case-sensitive and the pack has …"). Reading nothing (`max_bytes` 0) fails
/// before any data is read.
fn missing_reason(source: &mut PackSource, path: &str) -> String {
    match source.read(path, 0) {
        Err(problem) => problem.message,
        Ok(_) => "file not found in the pack".to_owned(),
    }
}

/// [`load`] and [`check_all`].
fn load_pack(
    path: &Path,
    origin: PackOrigin,
    sample_rate: u32,
    options: Options,
) -> Result<LoadedPack, PackError> {
    let stage = stage1(path, options.strict)?;
    let manifest_ok = !stage.has_errors();
    let Stage1 { mut source, manifest: m, problems, missing } = stage;
    // Without a file check there is nothing safe to decode; a pack with errors is decoded
    // only for validation.
    let missing = match missing {
        Some(missing) if manifest_ok || options.decode_despite_errors => missing,
        _ => return Err(PackError::new(path, problems)),
    };
    // With errors in pack.json, some references may be invalid or missing: skip those.
    let decodable = |path: &str| manifest::check_path(path).is_ok() && !missing.contains(path);

    let sound_files: BTreeSet<&str> = m
        .groups
        .values()
        .chain(m.keys.values())
        .flat_map(|set| set.press.iter().chain(&set.release))
        .map(String::as_str)
        .filter(|path| decodable(path))
        .collect();
    let preview = manifest::preview_file(&m).filter(|path| decodable(path));
    let preview_only = preview.filter(|p| !sound_files.contains(p));
    let jobs: Vec<Job> = sound_files
        .iter()
        .map(|&path| Job { path, in_bank: true })
        .chain(preview_only.map(|path| Job { path, in_bank: false }))
        .collect();

    let max_total_samples = options.max_total_samples;
    let ctx = DecodeContext {
        sample_rate,
        trim: m.trim_silence.unwrap_or(true),
        max_total_samples,
        total: AtomicUsize::new(0),
    };
    let outcomes = decode_all(&mut source, &jobs, &ctx, worker_count(jobs.len()));

    let mut errors = Vec::new();
    let mut decode_warnings = Vec::new();
    let mut samples: Vec<Box<[f32]>> = Vec::with_capacity(sound_files.len());
    let mut index: HashMap<&str, u32> = HashMap::with_capacity(sound_files.len());
    let mut preview_clip = None;
    for (job, outcome) in jobs.iter().zip(outcomes) {
        match outcome {
            Err(problem) => errors.push(problem),
            Ok(decoded) => {
                decode_warnings.extend(decoded.warning);
                if !job.in_bank {
                    preview_clip = decoded.samples;
                } else if let Some(clip) = decoded.samples {
                    index.insert(job.path, samples.len() as u32);
                    samples.push(clip);
                }
            }
        }
    }
    let total = ctx.total.into_inner();
    if total > max_total_samples {
        let seconds = |n: usize| n as f64 / f64::from(LIMIT_REFERENCE_RATE);
        errors.push(Problem::error(
            "pack",
            format!(
                "decoded audio is too large: {total} samples ({:.1} s) at {} kHz; the limit is \
                 {max_total_samples} samples ({:.1} s) at {} kHz, whatever the output rate",
                seconds(total),
                LIMIT_REFERENCE_RATE / 1000,
                seconds(max_total_samples),
                LIMIT_REFERENCE_RATE / 1000,
            ),
        ));
    }
    if !manifest_ok || !errors.is_empty() {
        // pack.json's errors, then the audio's, then all warnings (the sort is stable).
        let mut all = problems;
        all.extend(errors);
        all.extend(decode_warnings);
        all.sort_by_key(|p| Reverse(p.severity));
        return Err(PackError::new(path, all));
    }
    let info = pack_info(&m, path, origin);
    let mut warnings = problems;
    warnings.extend(decode_warnings);

    let mut map = SoundMap::new();
    let mut ids = Vec::new();
    for &key in Key::ALL {
        for action in [KeyAction::Down, KeyAction::Up] {
            ids.clear();
            // Every referenced file decoded (or loading failed above), so the lookup holds.
            ids.extend(manifest::resolve(&m, key, action).iter().map(|f| index[f.as_str()]));
            map.set(key, action, &ids);
        }
    }
    let gain = m.volume.unwrap_or(1.0);
    let preview = match preview.and_then(|p| index.get(p)) {
        Some(&i) => Some(samples[i as usize].clone()),
        None => preview_clip,
    }
    .map(|mut clip| {
        if gain != 1.0 {
            clip.iter_mut().for_each(|s| *s *= gain);
        }
        clip
    });
    let spec = m.variation.unwrap_or_default();
    let variation = Variation {
        pitch: spec.pitch.unwrap_or(manifest::DEFAULT_PITCH_VARIATION),
        volume: spec.volume.unwrap_or(manifest::DEFAULT_VOLUME_VARIATION),
    };
    let bank = SoundBank { samples, map, gain, variation };
    Ok(LoadedPack { info, bank, preview, warnings })
}

/// One distinct file to decode. `in_bank` is false only for a preview no key plays.
struct Job<'a> {
    path: &'a str,
    in_bank: bool,
}

struct DecodeContext {
    sample_rate: u32,
    trim: bool,
    max_total_samples: usize,
    /// Decoded samples so far, across all threads, counted at [`LIMIT_REFERENCE_RATE`]. Once
    /// over the limit, further output is counted (for the error message) but dropped, which
    /// bounds memory on hostile packs.
    total: AtomicUsize,
}

struct Decoded {
    /// `None` when dropped because the pack is over the total-size limit.
    samples: Option<Box<[f32]>>,
    warning: Option<Problem>,
}

type Outcome = Result<Decoded, Problem>;

fn worker_count(files: usize) -> usize {
    let available = thread::available_parallelism().map_or(1, NonZeroUsize::get);
    available.min(MAX_DECODE_THREADS).min(files / MIN_FILES_PER_THREAD).max(1)
}

/// Reads every job's file (the source needs `&mut`, so on this thread, in order) and decodes
/// them on `workers` threads. A bounded channel keeps at most a few files in flight, so a
/// large pack is never all in memory as encoded bytes. Outcomes are in job order.
fn decode_all(
    source: &mut PackSource,
    jobs: &[Job],
    ctx: &DecodeContext,
    workers: usize,
) -> Vec<Outcome> {
    let read = |source: &mut PackSource, job: &Job| source.read(job.path, MAX_AUDIO_FILE_BYTES);
    if workers <= 1 {
        return jobs
            .iter()
            .map(|job| read(source, job).and_then(|bytes| decode_guarded(job, bytes, ctx)))
            .collect();
    }

    let mut outcomes: Vec<Option<Outcome>> = jobs.iter().map(|_| None).collect();
    let (tx, rx) = mpsc::sync_channel::<(usize, Vec<u8>)>(workers);
    let rx = Mutex::new(rx);
    thread::scope(|s| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                s.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let next = rx.lock().unwrap_or_else(PoisonError::into_inner).recv();
                        let Ok((i, bytes)) = next else { break };
                        done.push((i, decode_guarded(&jobs[i], bytes, ctx)));
                    }
                    done
                })
            })
            .collect();
        for (i, job) in jobs.iter().enumerate() {
            match read(source, job) {
                Ok(bytes) => {
                    // Only fails if every worker is gone, which the join below reports.
                    if tx.send((i, bytes)).is_err() {
                        break;
                    }
                }
                Err(problem) => outcomes[i] = Some(Err(problem)),
            }
        }
        drop(tx);
        for handle in handles {
            let done = handle.join().unwrap_or_else(|payload| panic::resume_unwind(payload));
            for (i, outcome) in done {
                outcomes[i] = Some(outcome);
            }
        }
    });
    outcomes.into_iter().map(|o| o.expect("every job is either unreadable or decoded")).collect()
}

/// A decoder bug on one hostile file becomes that file's error instead of taking the whole
/// load down (in builds that unwind; release builds abort on panic).
fn decode_guarded(job: &Job, bytes: Vec<u8>, ctx: &DecodeContext) -> Outcome {
    panic::catch_unwind(AssertUnwindSafe(|| decode_file(job, bytes, ctx))).unwrap_or_else(|_| {
        Err(Problem::error(job.path, "the audio decoder failed on this file; try re-exporting it"))
    })
}

/// Decode, check the length, trim (before resampling, so the onset lands exactly on the
/// pre-roll at the output rate), resample.
fn decode_file(job: &Job, bytes: Vec<u8>, ctx: &DecodeContext) -> Outcome {
    let fail = |message: String| Problem::error(job.path, message);
    let extension = job.path.rsplit_once('.').map(|(_, ext)| ext.to_ascii_lowercase());
    let mut pcm = decode::decode(bytes, extension.as_deref().unwrap_or_default()).map_err(fail)?;
    if pcm.samples.len() as f64 > f64::from(MAX_SAMPLE_SECONDS) * f64::from(pcm.sample_rate) {
        return Err(fail(format!(
            "sound is {:.2} s long; sounds must be at most {MAX_SAMPLE_SECONDS} s",
            pcm.duration_seconds()
        )));
    }

    let mut warning = None;
    if ctx.trim {
        decode::trim_leading_silence(&mut pcm.samples, pcm.sample_rate);
    } else if job.in_bank {
        // Preview-only clips are not keystrokes; their lead-in delays nothing that matters.
        let silent = decode::leading_silence(&pcm.samples) as f32 / pcm.sample_rate as f32;
        if silent > LEADING_SILENCE_WARN_SECONDS {
            warning = Some(Problem::warning(
                job.path,
                format!(
                    "starts with {:.1} ms of silence, which delays every keystroke that plays \
                     it; trim the file or let TakTak do it (remove \"trim_silence\": false)",
                    silent * 1000.0
                ),
            ));
        }
    }

    // Exactly the length this file has when loaded at the reference rate, so the limit gives
    // the same verdict at every output rate.
    let counted = decode::resampled_len(pcm.samples.len(), pcm.sample_rate, LIMIT_REFERENCE_RATE);
    let samples = decode::resample(&pcm, ctx.sample_rate);
    drop(pcm);
    let before = ctx.total.fetch_add(counted, Ordering::Relaxed);
    let within_limit = before + counted <= ctx.max_total_samples;
    Ok(Decoded { samples: within_limit.then(|| samples.into_boxed_slice()), warning })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::Severity;
    use std::fs;
    use std::io::Cursor;
    use std::path::PathBuf;

    const RATE: u32 = 48_000;

    /// 16-bit mono WAV: `silence` zero samples, then a decaying 2 kHz burst of `len` samples.
    fn wav(rate: u32, silence: usize, len: usize, amp: f32) -> Vec<u8> {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut cursor = Cursor::new(Vec::new());
        let mut w = hound::WavWriter::new(&mut cursor, spec).unwrap();
        for _ in 0..silence {
            w.write_sample(0i16).unwrap();
        }
        for i in 0..len {
            let t = i as f32 / rate as f32;
            let s = amp * (std::f32::consts::TAU * 2000.0 * t).cos() * (-t / 0.01).exp();
            w.write_sample((s * 32767.0).round() as i16).unwrap();
        }
        w.finalize().unwrap();
        cursor.into_inner()
    }

    fn click() -> Vec<u8> {
        wav(RATE, 0, 2400, 0.8)
    }

    /// A folder pack from a pack.json body (fields after the required ones) and files.
    fn pack(fields: &str, files: &[(&str, Vec<u8>)]) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let json = format!(
            r#"{{"format": 1, "id": "test", "name": "Test", "author": "Me",
                 "license": "CC0-1.0", {fields}}}"#
        );
        fs::write(tmp.path().join("pack.json"), json).unwrap();
        for (rel, bytes) in files {
            let path = tmp.path().join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
        tmp
    }

    fn locations(problems: &[Problem]) -> Vec<(Severity, &str)> {
        problems.iter().map(|p| (p.severity, p.location.as_str())).collect()
    }

    #[test]
    fn references_cover_every_location_in_order() {
        let m = manifest::parse(
            br#"{"format": 1, "id": "a", "name": "A", "author": "B", "license": "MIT",
                 "preview": "p.wav",
                 "keys": {"KeyA": {"release": ["k.wav"]}},
                 "groups": {"space": {"press": ["s.wav", "t.wav"]},
                            "alphanumeric": {"press": ["a.wav"], "release": ["s.wav"]}}}"#,
        )
        .unwrap();
        let got: Vec<(String, &str)> = references(&m);
        let got: Vec<(&str, &str)> = got.iter().map(|(l, p)| (l.as_str(), *p)).collect();
        assert_eq!(
            got,
            [
                ("preview", "p.wav"),
                ("groups.alphanumeric.press[0]", "a.wav"),
                ("groups.alphanumeric.release[0]", "s.wav"),
                ("groups.space.press[0]", "s.wav"),
                ("groups.space.press[1]", "t.wav"),
                ("keys.KeyA.release[0]", "k.wav"),
            ]
        );
    }

    #[test]
    fn worker_count_is_bounded() {
        assert_eq!(worker_count(0), 1);
        assert_eq!(worker_count(MIN_FILES_PER_THREAD * 2 - 1), 1);
        let many = worker_count(10_000);
        assert!((1..=MAX_DECODE_THREADS).contains(&many), "{many}");
    }

    #[test]
    fn parallel_and_sequential_decoding_agree() {
        let mut files = Vec::new();
        let mut press = Vec::new();
        for i in 0..24 {
            let rel = format!("s/{i:02}.wav");
            // Different lengths and lead-ins, at a rate that needs resampling.
            files.push((rel.clone(), wav(44_100, i * 37, 1000 + i * 50, 0.5)));
            press.push(format!("{rel:?}"));
        }
        files.push(("s/broken.wav".to_owned(), b"RIFF nonsense".to_vec()));
        press.push("\"s/broken.wav\"".to_owned());
        let files: Vec<(&str, Vec<u8>)> =
            files.iter().map(|(r, b)| (r.as_str(), b.clone())).collect();
        let tmp =
            pack(&format!(r#""groups": {{"other": {{"press": [{}]}}}}"#, press.join(",")), &files);

        let run = |workers: usize| {
            let Stage1 { mut source, manifest: m, .. } = match stage1(tmp.path(), false) {
                Ok(stage) => stage,
                Err(e) => panic!("{e}"),
            };
            let paths: Vec<String> = manifest::referenced_files(&m).into_iter().collect();
            let jobs: Vec<Job> = paths.iter().map(|path| Job { path, in_bank: true }).collect();
            let ctx = DecodeContext {
                sample_rate: RATE,
                trim: true,
                max_total_samples: MAX_TOTAL_SAMPLES,
                total: AtomicUsize::new(0),
            };
            decode_all(&mut source, &jobs, &ctx, workers)
                .into_iter()
                .map(|o| o.map(|d| d.samples.unwrap()).map_err(|p| p.message))
                .collect::<Vec<_>>()
        };
        let sequential = run(1);
        assert_eq!(sequential.len(), 25);
        assert!(sequential[0].is_ok());
        assert!(sequential[24].as_ref().is_err_and(|e| e.starts_with("not a valid WAV file")));
        for workers in [2, 3, 8] {
            assert_eq!(run(workers), sequential, "{workers} workers");
        }
    }

    /// [`load`] with a small total-size limit.
    fn load_with_limit(path: &Path, rate: u32, limit: usize) -> Result<LoadedPack, PackError> {
        let options = Options { max_total_samples: limit, ..Options::PLAYBACK };
        load_pack(path, PackOrigin::User, rate, options)
    }

    #[test]
    fn total_size_limit_reports_the_total_and_keeps_other_errors() {
        let tmp = pack(
            r#""groups": {"other": {"press": ["a.wav", "b.wav", "c.wav", "bad.wav"]}}"#,
            &[
                ("a.wav", click()),
                ("b.wav", click()),
                ("c.wav", click()),
                ("bad.wav", b"not audio".to_vec()),
            ],
        );
        let err = load_with_limit(tmp.path(), RATE, 5000).err().unwrap();
        assert_eq!(
            locations(&err.problems),
            [
                (Severity::Error, "bad.wav"),
                (Severity::Error, "pack"),
                (Severity::Warning, "release")
            ]
        );
        assert_eq!(
            err.problems[1].message,
            "decoded audio is too large: 7200 samples (0.1 s) at 48 kHz; the limit is 5000 \
             samples (0.1 s) at 48 kHz, whatever the output rate"
        );

        let tmp = pack(
            r#""groups": {"other": {"press": ["a.wav", "b.wav"]}}"#,
            &[("a.wav", click()), ("b.wav", click())],
        );
        assert!(load_with_limit(tmp.path(), RATE, 4800).is_ok());
        assert!(load_with_limit(tmp.path(), RATE, 4799).is_err());
    }

    #[test]
    fn total_size_limit_does_not_depend_on_the_output_rate() {
        // 2400 + 2400 samples at 48 kHz, plus 2205 at 44.1 kHz (2400 at 48 kHz): 7200 in all.
        let tmp = pack(
            r#""groups": {"other": {"press": ["a.wav", "b.wav", "c.wav"]}}"#,
            &[("a.wav", click()), ("b.wav", click()), ("c.wav", wav(44_100, 0, 2205, 0.8))],
        );
        for rate in [44_100, 48_000, 88_200, 96_000, 192_000] {
            let loaded = load_with_limit(tmp.path(), rate, 7200)
                .unwrap_or_else(|e| panic!("{rate} Hz: {e}"));
            // The bank itself is at the output rate.
            let held: usize = loaded.bank.samples.iter().map(|s| s.len()).sum();
            let expected = 7200 * rate as usize / 48_000;
            assert!(held.abs_diff(expected) <= 2, "{rate} Hz: {held} samples");
            let err = load_with_limit(tmp.path(), rate, 7199).err().unwrap();
            assert!(
                err.problems[0].message.starts_with("decoded audio is too large: 7200 samples"),
                "{rate} Hz: {}",
                err.problems[0].message
            );
        }
    }

    #[test]
    fn check_all_reports_pack_json_and_audio_problems_together() {
        let tmp = pack(
            r#""groups": {"other": {"press": ["a.wav", "b.wav", "gone.wav", "bad\\path.wav"]},
                          "bogus": {"press": ["a.wav"]}}"#,
            &[("a.wav", click()), ("b.wav", b"not audio".to_vec())],
        );
        fs::write(
            tmp.path().join("pack.json"),
            fs::read_to_string(tmp.path().join("pack.json"))
                .unwrap()
                .replace(r#""id": "test""#, r#""id": "Bad_ID""#),
        )
        .unwrap();
        let err = check_all(tmp.path(), false, RATE).err().unwrap();
        assert_eq!(
            locations(&err.problems),
            [
                (Severity::Error, "id"),
                (Severity::Error, "groups.bogus"),
                (Severity::Error, "groups.other.press[3]"),
                (Severity::Error, "groups.other.press[2]"),
                (Severity::Error, "b.wav"),
                (Severity::Warning, "release"),
            ],
            "{err}"
        );
        assert!(err.problems[4].message.starts_with("not a valid WAV file"), "{err}");

        // Loading for playback only needs the verdict: nothing is decoded.
        let err = load(tmp.path(), PackOrigin::User, RATE).err().unwrap();
        assert!(!err.problems.iter().any(|p| p.location == "b.wav"), "{err}");
        assert_eq!(err.problems.len(), 5, "{err}");
    }

    #[test]
    fn check_all_is_strict_on_request_and_loads_clean_packs() {
        let tmp = pack(r#""groups": {"other": {"press": ["a.wav"]}}"#, &[("a.wav", click())]);
        let personal = |dir: &Path| {
            let json = fs::read_to_string(dir.join("pack.json")).unwrap();
            fs::write(dir.join("pack.json"), json.replace("CC0-1.0", "LicenseRef-Personal"))
                .unwrap();
        };
        assert_eq!(check_all(tmp.path(), true, RATE).unwrap().bank.samples.len(), 1);
        personal(tmp.path());
        assert!(check_all(tmp.path(), false, RATE).is_ok());
        let err = check_all(tmp.path(), true, RATE).err().unwrap();
        assert_eq!(locations(&err.problems)[0], (Severity::Error, "license"));
    }

    #[test]
    fn preview_only_file_is_not_in_the_bank() {
        let tmp = pack(
            r#""preview": "demo.wav", "groups": {"other": {"press": ["a.wav"]}}"#,
            &[("a.wav", click()), ("demo.wav", wav(RATE, 0, 9600, 0.5))],
        );
        let loaded = load(tmp.path(), PackOrigin::User, RATE).unwrap();
        assert_eq!(loaded.bank.samples.len(), 1);
        assert_eq!(loaded.preview.as_deref().map(<[f32]>::len), Some(9600));
    }

    #[test]
    fn preview_carries_the_pack_volume() {
        // Shared with a key sound: the bank keeps the file as decoded, the preview is scaled.
        let tmp = pack(
            r#""volume": 0.5, "preview": "a.wav", "groups": {"other": {"press": ["a.wav"]}}"#,
            &[("a.wav", click())],
        );
        let loaded = load(tmp.path(), PackOrigin::User, RATE).unwrap();
        assert_eq!(loaded.bank.gain, 0.5);
        let (bank, preview) = (&loaded.bank.samples[0], loaded.preview.unwrap());
        assert!(bank.iter().zip(&preview).all(|(b, p)| *p == b * 0.5));
        // A preview-only file likewise.
        let tmp = pack(
            r#""volume": 2.0, "preview": "demo.wav", "groups": {"other": {"press": ["a.wav"]}}"#,
            &[("a.wav", click()), ("demo.wav", wav(RATE, 0, 9600, 0.25))],
        );
        let preview = load(tmp.path(), PackOrigin::User, RATE).unwrap().preview.unwrap();
        let peak = preview.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!((peak - 0.5).abs() < 0.01, "{peak}");
    }

    #[test]
    fn leading_silence_warning_only_for_keystroke_sounds() {
        // 20 ms of silence in both; only the one keys play is worth a warning.
        let tmp = pack(
            r#""trim_silence": false, "preview": "demo.wav",
               "groups": {"other": {"press": ["a.wav"], "release": ["a.wav"]}}"#,
            &[("a.wav", wav(RATE, 960, 2400, 0.8)), ("demo.wav", wav(RATE, 960, 2400, 0.8))],
        );
        let loaded = load(tmp.path(), PackOrigin::User, RATE).unwrap();
        assert_eq!(locations(&loaded.warnings), [(Severity::Warning, "a.wav")]);
        assert!(loaded.warnings[0].message.starts_with("starts with 20.0 ms of silence"));
    }

    #[test]
    fn info_is_trimmed_and_located() {
        let tmp = pack(
            r#""version": " ", "description": "  Nice.  ",
               "groups": {"other": {"press": ["a.wav"]}}"#,
            &[("a.wav", click())],
        );
        let (info, _) = inspect(tmp.path(), PackOrigin::Bundled, true).unwrap();
        assert_eq!(info.location, PathBuf::from(tmp.path()));
        assert_eq!(info.origin, PackOrigin::Bundled);
        assert_eq!(info.version, None);
        assert_eq!(info.description.as_deref(), Some("Nice."));
    }

    #[test]
    fn newer_format_reports_only_that() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join("pack.json"),
            r#"{"format": 2, "id": "a", "name": "A", "author": "B", "license": "MIT",
                "groups": {"other": {"press": ["missing.wav"]}}}"#,
        )
        .unwrap();
        let err = inspect(tmp.path(), PackOrigin::User, false).unwrap_err();
        assert_eq!(locations(&err.problems), [(Severity::Error, "format")]);
    }

    #[test]
    fn read_manifest_skips_validation() {
        let tmp = pack(r#""groups": {"bogus": {"press": ["missing.wav"]}}"#, &[]);
        let m = read_manifest(tmp.path()).unwrap();
        assert!(m.groups.contains_key("bogus"));
        assert!(inspect(tmp.path(), PackOrigin::User, false).is_err());
    }
}
