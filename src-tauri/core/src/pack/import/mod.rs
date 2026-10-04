//! Importing Mechvibes sound packs into TakTak packs, for personal use.
//!
//! Mechvibes packs carry no license, so TakTak never bundles them; users import packs they
//! already have. [`import_mechvibes`] reads a Mechvibes, Mechvibes++ or MechvibesDX pack (a
//! folder or a `.zip`), cuts its audio into one 16-bit WAV per sound (mono, leading silence
//! trimmed, at most 2 s, faded out), maps its key codes to TakTak keys, renders a preview from
//! its own samples, and writes a valid pack folder with `license: "LicenseRef-Personal"`.
//! Formats and quirks: `docs/pack-format.md`, "Importing Mechvibes packs".
//!
//! Privacy: only pack contents (file names, key names from the config) are reported; nothing
//! is ever read from the keyboard.

pub mod audio;
pub mod keycodes;

mod input;
mod mechvibes;

use self::input::{Input, MAX_SOURCE_AUDIO_BYTES};
use self::mechvibes::{Clip, Plan, SplitHint};
use super::decode::{self, Pcm};
use super::manifest::{self, FORMAT_VERSION, Manifest, PERSONAL_LICENSE, SoundSet, VariationSpec};
use super::{PackError, load, printable};
use crate::audio::consistent_index;
use crate::input::KeyAction;
use crate::key::{Key, KeyGroup};
use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// Longest sprite sheet decoded (the longest known is 211 s).
const MAX_SOURCE_SECONDS: f32 = 400.0;
/// Output rate when the pack's files have different rates.
const MIXED_RATE_OUTPUT: u32 = 48_000;
/// Release splitting is used for a v1 sprite pack only when this share of its slices has a
/// clear release transient; otherwise keys keep the whole keystroke on press, as in Mechvibes.
/// (Of the official sprites, CherryMX Black PBT has the fewest: 78 %.)
const MIN_SPLIT_SHARE: f32 = 0.7;
/// In a split pack, a slice without a clear release transient is cut at this fraction of its
/// length: where the release starts in the median hand-cut Mechvibes slice.
const RELEASE_FRACTION: f32 = 0.65;
/// Files in the pack-wide fallback (`groups.alphanumeric`), taken from letter keys.
const FALLBACK_POOL: usize = 4;
const DEFAULT_AUTHOR: &str = "Unknown (imported from Mechvibes)";
const ID_PREFIX: &str = "mv-";
const MAX_NAME_CHARS: usize = 64;
const MAX_AUTHOR_CHARS: usize = 128;
const MAX_TEXT_CHARS: usize = 500;
const PREVIEW_FILE: &str = "preview.wav";
const BACKUP_CONFIG: &str = "config.json.v1.backup";
/// The made-up phrase the preview "types"; it ends with Enter.
const PREVIEW_KEYS: [Key; 10] = [
    Key::KeyH,
    Key::KeyE,
    Key::KeyL,
    Key::KeyL,
    Key::KeyO,
    Key::Space,
    Key::KeyT,
    Key::KeyA,
    Key::KeyK,
    Key::Enter,
];
/// Letter keys in the order they are tried for the fallback pool (home row first).
const FALLBACK_KEYS: [Key; 26] = [
    Key::KeyA,
    Key::KeyS,
    Key::KeyD,
    Key::KeyF,
    Key::KeyJ,
    Key::KeyK,
    Key::KeyL,
    Key::KeyE,
    Key::KeyR,
    Key::KeyU,
    Key::KeyI,
    Key::KeyO,
    Key::KeyT,
    Key::KeyN,
    Key::KeyG,
    Key::KeyH,
    Key::KeyW,
    Key::KeyQ,
    Key::KeyP,
    Key::KeyY,
    Key::KeyZ,
    Key::KeyX,
    Key::KeyC,
    Key::KeyV,
    Key::KeyB,
    Key::KeyM,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImportOptions {
    /// Replace an earlier import of the same pack (same `id` and source folder name).
    pub overwrite: bool,
    /// Split whole-keystroke clips (Mechvibes v1 sprite slices, MechvibesDX midpoint pairs)
    /// into press and release at the detected release transient.
    pub split_release: bool,
}

impl Default for ImportOptions {
    fn default() -> ImportOptions {
        ImportOptions { overwrite: false, split_release: true }
    }
}

/// The flavour of Mechvibes pack that was imported.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceFormat {
    /// `key_define_type: "single"`: `[start_ms, length_ms]` slices of one sprite file.
    MechvibesV1Sprite,
    /// `key_define_type: "multi"` (or `"multiple"`): one file per key.
    MechvibesV1Files,
    /// `version: 2`: `N-up` release keys, `sound`/`soundup` fallbacks, `{a-b}` ranges.
    MechvibesV2,
    /// `compatibility: true`: `0N` press and `00N` release keys.
    MechvibesPlusPlus { sprite: bool },
    /// `config_version: "2"` with `definitions`: W3C key names, `[start_ms, end_ms]` timings.
    MechvibesDx,
}

impl fmt::Display for SourceFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            SourceFormat::MechvibesV1Sprite => "Mechvibes v1, single sprite",
            SourceFormat::MechvibesV1Files => "Mechvibes v1, one file per key",
            SourceFormat::MechvibesV2 => "Mechvibes v2",
            SourceFormat::MechvibesPlusPlus { sprite: true } => "Mechvibes++ compat, sprite",
            SourceFormat::MechvibesPlusPlus { sprite: false } => "Mechvibes++ compat, files",
            SourceFormat::MechvibesDx => "MechvibesDX config v2",
        })
    }
}

/// A `defines`/`definitions` key that was not imported, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkippedKey {
    pub key: String,
    pub reason: String,
}

/// What release splitting did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SplitSummary {
    /// Whole-keystroke clips that could be split.
    pub eligible: usize,
    /// Split at a detected release transient.
    pub detected: usize,
    /// Split at a fallback point (MechvibesDX's boundary, or the slice midpoint).
    pub fallback: usize,
    /// Kept whole (splitting off, or too few clear transients in the pack).
    pub kept_whole: usize,
}

#[derive(Clone, Debug)]
pub struct ImportReport {
    pub id: String,
    pub name: String,
    /// The pack folder written.
    pub path: PathBuf,
    /// The source folder or zip file name (also the pack's `source`).
    pub source: String,
    pub format: SourceFormat,
    pub sample_rate: u32,
    /// Keys with a sound of their own, by `KeyboardEvent.code` name.
    pub keys_mapped: Vec<String>,
    /// How many of them also have a release sound of their own.
    pub keys_with_release: usize,
    pub split: SplitSummary,
    pub skipped: Vec<SkippedKey>,
    /// Files the config names that are not in the pack.
    pub missing_files: Vec<String>,
    /// Files that are present but could not be decoded, with the reason.
    pub unreadable_files: Vec<(String, String)>,
    pub warnings: Vec<String>,
    /// WAV files written (sounds, without the preview).
    pub sounds_written: usize,
    /// The loader's warnings for the written pack (the personal license, …).
    pub pack_warnings: Vec<String>,
    /// Whether an earlier import was replaced.
    pub replaced: bool,
}

impl fmt::Display for ImportReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "{} -> {} ({}){}",
            printable(&self.source),
            self.id,
            printable(&self.path.display().to_string()),
            if self.replaced { " [replaced]" } else { "" }
        )?;
        writeln!(f, "  name:     {}", printable(&self.name))?;
        writeln!(f, "  format:   {}, {} Hz", self.format, self.sample_rate)?;
        writeln!(
            f,
            "  keys:     {} mapped ({} with their own release), {} skipped",
            self.keys_mapped.len(),
            self.keys_with_release,
            self.skipped.len()
        )?;
        let s = &self.split;
        if s.eligible > 0 {
            writeln!(
                f,
                "  release:  {} whole-keystroke clips: {} split at the detected release, {} at \
                 a fallback point, {} kept whole",
                s.eligible, s.detected, s.fallback, s.kept_whole
            )?;
        }
        writeln!(f, "  files:    {} sounds + preview", self.sounds_written)?;
        for k in &self.skipped {
            writeln!(f, "  skipped   {:?}: {}", printable(&k.key), printable(&k.reason))?;
        }
        for m in &self.missing_files {
            writeln!(f, "  missing   {:?}", printable(m))?;
        }
        for (file, reason) in &self.unreadable_files {
            writeln!(f, "  unreadable {:?}: {}", printable(file), printable(reason))?;
        }
        for w in self.warnings.iter().chain(&self.pack_warnings) {
            writeln!(f, "  warning   {}", printable(w))?;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum ImportError {
    Io {
        path: PathBuf,
        message: String,
    },
    /// Not a Mechvibes pack (no config.json, several packs in one zip, …).
    NotAPack(String),
    InvalidConfig(String),
    /// A pack TakTak cannot import (mouse packs, unknown config versions, …).
    Unsupported(String),
    /// Nothing playable was left (missing or undecodable files).
    NoSounds(String),
    /// The pack was imported before; pass `overwrite` to replace it.
    AlreadyImported {
        id: String,
        path: PathBuf,
    },
    /// The written pack failed validation (an importer bug); nothing was installed.
    Invalid(PackError),
}

impl ImportError {
    fn io(path: &Path, e: std::io::Error) -> ImportError {
        ImportError::Io { path: path.to_path_buf(), message: e.to_string() }
    }
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportError::Io { path, message } => {
                write!(f, "{}: {}", printable(&path.display().to_string()), message)
            }
            ImportError::NotAPack(m) => write!(f, "not a Mechvibes pack: {}", printable(m)),
            ImportError::InvalidConfig(m) => write!(f, "invalid config: {}", printable(m)),
            ImportError::Unsupported(m) => write!(f, "unsupported: {}", printable(m)),
            ImportError::NoSounds(m) => write!(f, "no usable sounds: {}", printable(m)),
            ImportError::AlreadyImported { id, path } => write!(
                f,
                "already imported as {id} ({}); use overwrite to replace it",
                printable(&path.display().to_string())
            ),
            ImportError::Invalid(e) => write!(f, "the imported pack is invalid (a bug): {e}"),
        }
    }
}

impl std::error::Error for ImportError {}

/// Findings collected while reading a config.
#[derive(Default)]
pub(crate) struct Notes {
    skipped: Vec<SkippedKey>,
    missing: BTreeSet<String>,
    warnings: Vec<String>,
}

impl Notes {
    fn warn(&mut self, message: impl Into<String>) {
        let message = message.into();
        if !self.warnings.contains(&message) {
            self.warnings.push(message);
        }
    }

    fn skip(&mut self, key: &str, reason: &str) {
        self.skipped.push(SkippedKey { key: key.to_owned(), reason: reason.to_owned() });
    }

    fn missing(&mut self, file: &str) {
        self.missing.insert(file.trim().to_owned());
    }
}

/// Imports the Mechvibes pack at `src` (a folder or a `.zip`) into `dest_packs_dir/<id>`.
///
/// The id is `mv-<slug of the pack name>`, made unique with a numeric suffix if another pack
/// in `dest_packs_dir` already uses it. An earlier import of the same pack (same id and same
/// source folder name) is an error unless `opts.overwrite`. The pack is written to a hidden
/// temporary folder, validated with the loader, and only then moved into place, so the
/// hot-reloading registry never sees a half-written pack.
pub fn import_mechvibes(
    src: &Path,
    dest_packs_dir: &Path,
    opts: ImportOptions,
) -> Result<ImportReport, ImportError> {
    let mut input = Input::open(src)?;
    let mut notes = Notes::default();
    let config_name = input.config_name.clone();
    let mut plan = None;
    if let Some(backup) = sibling(&config_name, BACKUP_CONFIG).and_then(|b| input.find_exact_ci(&b))
    {
        let mut backup_notes = Notes::default();
        if let Ok(p) = mechvibes::plan(&mut input, &backup, &opts, &mut backup_notes) {
            notes = backup_notes;
            notes.warn(format!(
                "MechvibesDX converted this pack; imported the original Mechvibes config ({backup})"
            ));
            plan = Some(p);
        }
    }
    let plan = match plan {
        Some(p) => p,
        None => mechvibes::plan(&mut input, &config_name, &opts, &mut notes)?,
    };
    let built = build(&mut input, plan, &opts, &mut notes)?;
    install(&input, built, dest_packs_dir, opts, notes)
}

fn sibling(config: &str, name: &str) -> Option<String> {
    Some(match config.rsplit_once('/') {
        Some((dir, _)) => format!("{dir}/{name}"),
        None => name.to_owned(),
    })
}

/// An imported pack in memory, ready to write.
struct Built {
    plan_name: Option<String>,
    author: Option<String>,
    description: Option<String>,
    format: SourceFormat,
    rate: u32,
    volume: Option<f32>,
    pitch: Option<f32>,
    /// Distinct sounds, with their pack paths once named.
    sounds: Vec<Vec<f32>>,
    paths: Vec<String>,
    keys: BTreeMap<Key, (Vec<usize>, Vec<usize>)>,
    fallback: (Vec<usize>, Vec<usize>),
    split: SplitSummary,
    unreadable: Vec<(String, String)>,
}

/// The audio format of a file, by content first (Mechvibes packs have misnamed and
/// extension-less files), else by extension.
fn sniff(bytes: &[u8], file: &str) -> Result<&'static str, String> {
    let at = |range: std::ops::Range<usize>| bytes.get(range).unwrap_or(&[]);
    if at(0..4) == b"RIFF" && at(8..12) == b"WAVE" {
        return Ok("wav");
    }
    if at(0..4) == b"OggS" {
        return Ok("ogg");
    }
    if at(0..3) == b"ID3" || (bytes.len() > 2 && bytes[0] == 0xFF && bytes[1] & 0xE0 == 0xE0) {
        return Ok("mp3");
    }
    let convert = "convert it to WAV, Ogg Vorbis or MP3";
    if at(0..4) == b"fLaC" {
        return Err(format!("FLAC audio is not supported; {convert}"));
    }
    if at(4..8) == b"ftyp" {
        return Err(format!("AAC/M4A audio is not supported; {convert}"));
    }
    if at(0..4) == b"FORM" {
        return Err(format!("AIFF audio is not supported; {convert}"));
    }
    if at(0..4) == b"RF64" || at(0..4) == b"riff" {
        return Err(format!("RF64/W64 audio is not supported; {convert}"));
    }
    match file.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).as_deref() {
        Some("wav") => Ok("wav"),
        Some("ogg") => Ok("ogg"),
        Some("mp3") => Ok("mp3"),
        Some("m4a" | "aac" | "mp4") => Err(format!("AAC/M4A audio is not supported; {convert}")),
        _ => Err("unrecognized audio format".into()),
    }
}

/// Sample bounds of a clip in `pcm`: `[a, b)`, clamped to the file.
fn bounds(pcm: &Pcm, clip: &Clip) -> Option<(usize, usize)> {
    let len = pcm.samples.len();
    let Some((start, end)) = clip.range else { return Some((0, len)) };
    let at = |us: i64| ((us.max(0) as f64) * f64::from(pcm.sample_rate) / 1e6).round() as usize;
    let (a, b) = (at(start), at(end).min(len));
    (a < b).then_some((a, b))
}

/// The analysis of one whole-keystroke clip.
struct Cut {
    start: usize,
    end: usize,
    detected: Option<usize>,
}

fn build(
    input: &mut Input,
    plan: Plan,
    opts: &ImportOptions,
    notes: &mut Notes,
) -> Result<Built, ImportError> {
    // Decode every referenced file once.
    let all_clips: BTreeSet<&Clip> = plan
        .keys
        .values()
        .chain(std::iter::once(&plan.fallback))
        .flat_map(|k| k.press.iter().chain(&k.release))
        .collect();
    let files: BTreeSet<&str> = all_clips.iter().map(|c| c.file.as_str()).collect();
    let mut pcm: HashMap<String, Pcm> = HashMap::new();
    let mut unreadable = Vec::new();
    for &file in &files {
        let decoded = input.read(file, MAX_SOURCE_AUDIO_BYTES).and_then(|bytes| {
            let ext = sniff(&bytes, file)?;
            decode::decode_long(bytes, ext, MAX_SOURCE_SECONDS)
        });
        match decoded {
            Ok(p) => {
                pcm.insert(file.to_owned(), p);
            }
            Err(reason) => unreadable.push((file.to_owned(), reason)),
        }
    }
    let rates: BTreeSet<u32> = pcm.values().map(|p| p.sample_rate).collect();
    let rate = match (rates.first(), rates.len()) {
        (Some(&r), 1) => r,
        _ => MIXED_RATE_OUTPUT,
    };

    // Starts of every slice per file, for the release extension's stop rule.
    let mut starts: HashMap<&str, Vec<usize>> = HashMap::new();
    for clip in &all_clips {
        if let (Some(p), Some(_)) = (pcm.get(&clip.file), clip.range)
            && let Some((a, _)) = bounds(p, clip)
        {
            starts.entry(clip.file.as_str()).or_default().push(a);
        }
    }
    starts.values_mut().for_each(|v| {
        v.sort_unstable();
        v.dedup();
    });

    // Analyse whole-keystroke clips for a release transient.
    let mut cuts: HashMap<Clip, Cut> = HashMap::new();
    for kp in plan.keys.values() {
        let (Some(_), [clip]) = (kp.split, kp.press.as_slice()) else { continue };
        if cuts.contains_key(clip) {
            continue;
        }
        let Some(p) = pcm.get(&clip.file) else { continue };
        let Some((a, b)) = bounds(p, clip) else { continue };
        let Some(lead) = audio::trimmed_start(&p.samples[a..b], p.sample_rate) else { continue };
        let start = a + lead;
        let detected = if opts.split_release {
            audio::find_release_cut(&p.samples[start..b], p.sample_rate).map(|c| start + c)
        } else {
            None
        };
        cuts.insert(clip.clone(), Cut { start, end: b, detected });
    }
    let detect_clips: Vec<&Clip> = plan
        .keys
        .values()
        .filter(|k| k.split == Some(SplitHint::Detect))
        .flat_map(|k| k.press.first())
        .filter(|c| cuts.contains_key(*c))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let detect_ok = detect_clips.iter().filter(|c| cuts[**c].detected.is_some()).count();
    let split_v1 = opts.split_release
        && !detect_clips.is_empty()
        && detect_ok as f32 >= MIN_SPLIT_SHARE * detect_clips.len() as f32;
    if opts.split_release && !detect_clips.is_empty() && !split_v1 {
        notes.warn(format!(
            "only {detect_ok} of {} sprite slices have a clear release transient; keys play \
             the whole keystroke on press, as in Mechvibes",
            detect_clips.len()
        ));
    }

    let mut r = Renderer {
        pcm: &pcm,
        cuts: &cuts,
        starts: &starts,
        rate,
        split_v1,
        sounds: Vec::new(),
        by_hash: HashMap::new(),
        rendered: HashMap::new(),
        split: SplitSummary::default(),
        truncated: 0,
        silent: 0,
        overlong: 0,
    };
    let mut keys: BTreeMap<Key, (Vec<usize>, Vec<usize>)> = BTreeMap::new();
    for (&key, kp) in &plan.keys {
        let mut press = Vec::new();
        let mut release = Vec::new();
        for clip in &kp.press {
            let hint = if kp.press.len() == 1 { kp.split } else { None };
            let (p, rel) = r.render(clip, hint);
            press.extend(p);
            release.extend(rel);
        }
        for clip in &kp.release {
            release.extend(r.render(clip, None).0);
        }
        dedup_keep_order(&mut press);
        dedup_keep_order(&mut release);
        if !press.is_empty() || !release.is_empty() {
            keys.insert(key, (press, release));
        }
    }
    let mut fallback = (Vec::new(), Vec::new());
    for clip in &plan.fallback.press {
        fallback.0.extend(r.render(clip, None).0);
    }
    for clip in &plan.fallback.release {
        fallback.1.extend(r.render(clip, None).0);
    }
    dedup_keep_order(&mut fallback.0);
    dedup_keep_order(&mut fallback.1);
    let Renderer { sounds, split, truncated, silent, overlong: overlong_slices, .. } = r;

    if truncated > 0 {
        notes.warn(format!(
            "{truncated} sounds were longer than {:.2} s and were cut short",
            audio::MAX_CLIP_SECONDS
        ));
    }
    if overlong_slices > 0 {
        notes.warn(format!(
            "{overlong_slices} slices run past the end of their audio file and were shortened \
             or dropped"
        ));
    }
    if silent > 0 {
        notes.warn(format!("{silent} clips are silent and were dropped"));
    }

    // The pack-wide fallback: the pack's own (v2), else a few letter keys' sounds.
    if fallback.0.is_empty() {
        fallback.0 = pool(&keys, |(press, _)| press.first().copied());
    }
    if fallback.1.is_empty() {
        fallback.1 = pool(&keys, |(_, release)| release.first().copied());
    }
    if fallback.0.is_empty() {
        let mut detail = format!(
            "none of the pack's sounds could be used ({} missing, {} unreadable)",
            notes.missing.len(),
            unreadable.len()
        );
        if !input.has_audio_candidates() {
            detail.push_str(
                "; the pack holds no audio files at all (mechvibes.com \"v2\" downloads contain \
                 only config.json: import the pack folder from the Mechvibes app's custom \
                 folder instead)",
            );
        }
        return Err(ImportError::NoSounds(detail));
    }

    Ok(Built {
        plan_name: plan.name,
        author: plan.author,
        description: plan.description,
        format: plan.format,
        rate,
        volume: plan.volume,
        pitch: plan.pitch,
        paths: Vec::new(),
        sounds,
        keys,
        fallback,
        split,
        unreadable,
    })
}

/// Cuts clips out of the decoded files and collects the distinct sounds.
struct Renderer<'a> {
    pcm: &'a HashMap<String, Pcm>,
    cuts: &'a HashMap<Clip, Cut>,
    /// Slice starts per file, sorted: a release extension stops before the next slice.
    starts: &'a HashMap<&'a str, Vec<usize>>,
    /// Output rate.
    rate: u32,
    /// Whether the pack's v1 sprite slices are split (enough clear release transients).
    split_v1: bool,
    sounds: Vec<Vec<f32>>,
    by_hash: HashMap<u64, Vec<usize>>,
    /// (press, release) sound ids per clip and split decision.
    rendered: HashMap<(Clip, bool), (Option<usize>, Option<usize>)>,
    split: SplitSummary,
    truncated: usize,
    silent: usize,
    overlong: usize,
}

impl Renderer<'_> {
    /// Resamples, caps and fades `samples`, and returns the id of that sound (an identical
    /// sound already rendered is reused).
    fn add(&mut self, mut samples: Vec<f32>, from_rate: u32) -> usize {
        if from_rate != self.rate {
            samples = decode::resample(&Pcm { sample_rate: from_rate, samples }, self.rate);
        }
        if audio::finish(&mut samples, self.rate) {
            self.truncated += 1;
        }
        let quantized: Vec<i16> =
            samples.iter().map(|s| (s.clamp(-1.0, 1.0) * 32767.0).round() as i16).collect();
        let mut h = DefaultHasher::new();
        quantized.hash(&mut h);
        let bucket = self.by_hash.entry(h.finish()).or_default();
        if let Some(&id) = bucket.iter().find(|&&id| self.sounds[id] == samples) {
            return id;
        }
        self.sounds.push(samples);
        bucket.push(self.sounds.len() - 1);
        self.sounds.len() - 1
    }

    /// Where to split a whole-keystroke clip `[a, b)`, as `(press start, cut)`, counting the
    /// decision in the summary. `None` keeps the clip whole.
    fn split_point(
        &mut self,
        clip: &Clip,
        hint: SplitHint,
        a: usize,
        b: usize,
        rate: u32,
    ) -> Option<(usize, usize)> {
        let c = self.cuts.get(clip)?;
        self.split.eligible += 1;
        let detected = c.detected.map(|d| (d, true));
        let point = match hint {
            SplitHint::Detect if !self.split_v1 => None,
            SplitHint::Detect => {
                detected.or(Some((a + ((b - a) as f32 * RELEASE_FRACTION) as usize, false)))
            }
            SplitHint::Boundary(us) => {
                detected.or(Some((((us as f64) * f64::from(rate) / 1e6).round() as usize, false)))
            }
        };
        match point {
            Some((at, detected)) if at > c.start && at < c.end => {
                if detected {
                    self.split.detected += 1;
                } else {
                    self.split.fallback += 1;
                }
                Some((c.start, at))
            }
            _ => {
                self.split.kept_whole += 1;
                None
            }
        }
    }

    /// The (press, release) sounds of a clip; a release only when `hint` splits it.
    fn render(&mut self, clip: &Clip, hint: Option<SplitHint>) -> (Option<usize>, Option<usize>) {
        let key = (clip.clone(), hint.is_some());
        if let Some(done) = self.rendered.get(&key) {
            return *done;
        }
        let result = self.render_new(clip, hint);
        self.rendered.insert(key, result);
        result
    }

    fn render_new(
        &mut self,
        clip: &Clip,
        hint: Option<SplitHint>,
    ) -> (Option<usize>, Option<usize>) {
        let Some(p) = self.pcm.get(&clip.file) else { return (None, None) };
        let (samples, rate) = (&p.samples, p.sample_rate);
        let Some((a, b)) = bounds(p, clip) else {
            self.overlong += 1;
            return (None, None);
        };
        if let Some((_, end)) = clip.range
            && ((end as f64) * f64::from(rate) / 1e6).round() as usize > samples.len()
        {
            self.overlong += 1;
        }
        match hint.and_then(|h| self.split_point(clip, h, a, b, rate)) {
            Some((start, at)) => {
                let next = self
                    .starts
                    .get(clip.file.as_str())
                    .and_then(|v| v.iter().copied().find(|&s| s > a));
                let end = audio::extended_release_end(samples, rate, b, next);
                let tail = &samples[at..end];
                let release = audio::trimmed_start(tail, rate)
                    .map(|lead| self.add(tail[lead..].to_vec(), rate));
                (Some(self.add(samples[start..at].to_vec(), rate)), release)
            }
            None => match audio::trimmed_start(&samples[a..b], rate) {
                Some(lead) => (Some(self.add(samples[a + lead..b].to_vec(), rate)), None),
                None => {
                    self.silent += 1;
                    (None, None)
                }
            },
        }
    }
}

fn dedup_keep_order(ids: &mut Vec<usize>) {
    let mut seen = BTreeSet::new();
    ids.retain(|id| seen.insert(*id));
}

/// Up to [`FALLBACK_POOL`] distinct sounds, from letter keys (home row first), else from
/// other alphanumeric keys, else from any key.
fn pool(
    keys: &BTreeMap<Key, (Vec<usize>, Vec<usize>)>,
    pick: impl Fn(&(Vec<usize>, Vec<usize>)) -> Option<usize>,
) -> Vec<usize> {
    let letters = FALLBACK_KEYS.iter().filter_map(|k| keys.get(k));
    let alnum = keys.iter().filter(|(k, _)| k.group() == KeyGroup::Alphanumeric).map(|(_, v)| v);
    let mut out = Vec::new();
    for sets in [letters.collect::<Vec<_>>(), alnum.collect(), keys.values().collect()] {
        for set in sets {
            if let Some(id) = pick(set)
                && !out.contains(&id)
                && out.len() < FALLBACK_POOL
            {
                out.push(id);
            }
        }
        if !out.is_empty() {
            break;
        }
    }
    out
}

/// Names every sound after its first user: `sounds/<Key>-press.wav`, `-release`, or
/// `sounds/fallback-press-N.wav` for sounds only the fallback plays.
fn name_sounds(built: &mut Built) {
    let mut paths: Vec<Option<String>> = vec![None; built.sounds.len()];
    let mut taken = BTreeSet::new();
    let mut name = |id: usize, stem: String, paths: &mut Vec<Option<String>>| {
        if paths[id].is_some() {
            return;
        }
        let mut path = format!("sounds/{stem}.wav");
        let mut n = 2;
        while !taken.insert(path.clone()) {
            path = format!("sounds/{stem}-{n}.wav");
            n += 1;
        }
        paths[id] = Some(path);
    };
    for (key, (press, release)) in &built.keys {
        for (role, ids) in [("press", press), ("release", release)] {
            for (i, &id) in ids.iter().enumerate() {
                let stem = match ids.len() {
                    1 => format!("{}-{role}", key.code_name()),
                    _ => format!("{}-{role}-{}", key.code_name(), i + 1),
                };
                name(id, stem, &mut paths);
            }
        }
    }
    for (role, ids) in [("press", &built.fallback.0), ("release", &built.fallback.1)] {
        for (i, &id) in ids.iter().enumerate() {
            name(id, format!("fallback-{role}-{}", i + 1), &mut paths);
        }
    }
    built.paths = paths.into_iter().map(Option::unwrap_or_default).collect();
}

/// `text` without control characters, trimmed, at most `max` characters.
fn clean(text: &str, max: usize) -> String {
    let replaced: String =
        text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>();
    let truncated: String = replaced.trim().chars().take(max).collect();
    truncated.trim_end().to_owned()
}

/// A lowercase ASCII slug (`a-z0-9` runs joined by `-`), at most `max` characters.
fn slug(text: &str, max: usize) -> String {
    let mut out = String::new();
    for c in text.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let truncated: String = out.chars().take(max).collect();
    truncated.trim_matches('-').to_owned()
}

fn origin_stem(origin: &str) -> &str {
    origin.strip_suffix(".zip").or_else(|| origin.strip_suffix(".ZIP")).unwrap_or(origin)
}

fn manifest_for(built: &Built, id: &str, source: &str) -> (Manifest, Vec<String>) {
    let mut notes = Vec::new();
    let raw_name = built.plan_name.clone().unwrap_or_else(|| origin_stem(source).to_owned());
    let mut name = clean(&raw_name, MAX_NAME_CHARS);
    if name.is_empty() {
        name = clean(origin_stem(source), MAX_NAME_CHARS);
    }
    if name.is_empty() {
        name = "Imported Mechvibes pack".into();
    }
    let full_name = clean(&raw_name, usize::MAX);
    let mut description = format!(
        "Imported from the Mechvibes pack \"{}\" for personal use; not for redistribution.",
        clean(source, 100)
    );
    if full_name.chars().count() > MAX_NAME_CHARS {
        description.push_str(&format!(" Full name: {full_name}."));
        notes.push(format!("the pack name is longer than {MAX_NAME_CHARS} characters; shortened"));
    }
    if let Some(original) = &built.description {
        description.push_str(&format!(" Original description: {}", clean(original, usize::MAX)));
    }
    let author = built
        .author
        .as_deref()
        .map(|a| clean(a, MAX_AUTHOR_CHARS))
        .filter(|a| !a.is_empty())
        .unwrap_or_else(|| DEFAULT_AUTHOR.to_owned());

    let set = |press: &[usize], release: &[usize]| SoundSet {
        press: press.iter().map(|&i| built.paths[i].clone()).collect(),
        release: release.iter().map(|&i| built.paths[i].clone()).collect(),
        extra: BTreeMap::new(),
    };
    let alnum = set(&built.fallback.0, &built.fallback.1);
    let keys = built
        .keys
        .iter()
        .map(|(key, (press, release))| (key.code_name().to_owned(), set(press, release)))
        // A key that would resolve to exactly the same files through the fallback is left out.
        .filter(|(_, s)| !(s.press == alnum.press && s.release == alnum.release))
        .collect();
    let manifest = Manifest {
        format: FORMAT_VERSION,
        id: id.to_owned(),
        name,
        version: None,
        author,
        license: PERSONAL_LICENSE.to_owned(),
        description: Some(clean(&description, MAX_TEXT_CHARS)),
        source: Some(clean(source, MAX_TEXT_CHARS)),
        attribution: None,
        preview: Some(PREVIEW_FILE.to_owned()),
        volume: built.volume,
        trim_silence: Some(true),
        variation: built.pitch.map(|p| VariationSpec { pitch: Some(p), volume: None }),
        groups: BTreeMap::from([("alphanumeric".to_owned(), alnum)]),
        keys,
        extra: BTreeMap::new(),
    };
    (manifest, notes)
}

fn preview(built: &Built, manifest: &Manifest) -> Vec<f32> {
    let by_path: HashMap<&str, usize> =
        built.paths.iter().enumerate().map(|(i, p)| (p.as_str(), i)).collect();
    let pick = |key: Key, action: KeyAction| -> Option<&[f32]> {
        let files = manifest::resolve(manifest, key, action);
        if files.is_empty() {
            return None;
        }
        let file = &files[consistent_index(key, files.len())];
        by_path.get(file.as_str()).map(|&i| built.sounds[i].as_slice())
    };
    let strokes: Vec<(&[f32], Option<&[f32]>)> = PREVIEW_KEYS
        .iter()
        .filter_map(|&k| Some((pick(k, KeyAction::Down)?, pick(k, KeyAction::Up))))
        .collect();
    audio::mix_preview(&strokes, built.rate)
}

/// Ids (and their pack's `source`) of the packs already in `dest`.
fn existing_packs(dest: &Path) -> HashMap<String, (PathBuf, Option<String>)> {
    let mut out = HashMap::new();
    let Ok(entries) = fs::read_dir(dest) else { return out };
    for entry in entries.flatten() {
        let path = entry.path();
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if let Ok(m) = load::read_manifest(&path) {
            out.insert(m.id.clone(), (path, m.source.clone()));
        }
    }
    out
}

/// Picks the pack id and whether an earlier import is replaced.
fn choose_id(
    base: &str,
    source: &str,
    dest: &Path,
    overwrite: bool,
) -> Result<(String, bool), ImportError> {
    let existing = existing_packs(dest);
    for n in 1..=999 {
        let candidate = if n == 1 { base.to_owned() } else { format!("{base}-{n}") };
        let folder = dest.join(&candidate);
        let holder = existing.get(&candidate);
        if holder.is_none() && fs::symlink_metadata(&folder).is_err() {
            return Ok((candidate, false));
        }
        let same_pack = holder.is_some_and(|(path, src)| {
            path == &folder && src.as_deref() == Some(clean(source, MAX_TEXT_CHARS).as_str())
        });
        if same_pack {
            return if overwrite {
                Ok((candidate, true))
            } else {
                Err(ImportError::AlreadyImported { id: candidate, path: folder })
            };
        }
    }
    Err(ImportError::Io {
        path: dest.to_path_buf(),
        message: format!("no free pack id starting with {base}"),
    })
}

fn install(
    input: &Input,
    mut built: Built,
    dest: &Path,
    opts: ImportOptions,
    notes: Notes,
) -> Result<ImportReport, ImportError> {
    let source = input.origin_name.clone();
    let name_for_id = built.plan_name.clone().unwrap_or_else(|| origin_stem(&source).to_owned());
    let max_slug = 64 - ID_PREFIX.len() - 4;
    let mut base_slug = slug(&name_for_id, max_slug);
    if base_slug.is_empty() {
        base_slug = slug(origin_stem(&source), max_slug);
    }
    if base_slug.is_empty() {
        base_slug = "pack".into();
    }
    let base = format!("{ID_PREFIX}{base_slug}");
    fs::create_dir_all(dest).map_err(|e| ImportError::io(dest, e))?;
    let (id, replaced) = choose_id(&base, &source, dest, opts.overwrite)?;

    name_sounds(&mut built);
    let (manifest, manifest_notes) = manifest_for(&built, &id, &source);
    let preview = preview(&built, &manifest);

    let tmp = dest.join(format!(".{id}.importing-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    let write = || -> std::io::Result<()> {
        fs::create_dir_all(tmp.join("sounds"))?;
        // Sounds every key entry was dropped for (identical to the fallback) are still
        // referenced by the fallback; anything unreferenced is not written.
        let referenced = manifest::referenced_files(&manifest);
        for (samples, path) in built.sounds.iter().zip(&built.paths) {
            if referenced.contains(path) {
                fs::write(tmp.join(path), audio::wav_bytes(samples, built.rate))?;
            }
        }
        fs::write(tmp.join(PREVIEW_FILE), audio::wav_bytes(&preview, built.rate))?;
        let mut json = serde_json::to_vec_pretty(&manifest).map_err(std::io::Error::other)?;
        json.push(b'\n');
        fs::write(tmp.join("pack.json"), json)
    };
    if let Err(e) = write() {
        let _ = fs::remove_dir_all(&tmp);
        return Err(ImportError::io(&tmp, e));
    }
    let checked = match load::check_all(&tmp, false, decode_check_rate()) {
        Ok(loaded) => loaded,
        Err(e) => {
            let _ = fs::remove_dir_all(&tmp);
            return Err(ImportError::Invalid(e));
        }
    };

    let final_dir = dest.join(&id);
    if replaced {
        let old = dest.join(format!(".{id}.replaced-{}", std::process::id()));
        let _ = fs::remove_dir_all(&old);
        fs::rename(&final_dir, &old).map_err(|e| ImportError::io(&final_dir, e))?;
        if let Err(e) = fs::rename(&tmp, &final_dir) {
            let _ = fs::rename(&old, &final_dir);
            let _ = fs::remove_dir_all(&tmp);
            return Err(ImportError::io(&final_dir, e));
        }
        let _ = fs::remove_dir_all(&old);
    } else if let Err(e) = fs::rename(&tmp, &final_dir) {
        let _ = fs::remove_dir_all(&tmp);
        return Err(ImportError::io(&final_dir, e));
    }

    let referenced = manifest::referenced_files(&manifest);
    let sounds_written = built.paths.iter().filter(|p| referenced.contains(*p)).count();
    let mut warnings = notes.warnings;
    warnings.extend(manifest_notes);
    Ok(ImportReport {
        id,
        name: manifest.name.clone(),
        path: final_dir,
        source,
        format: built.format,
        sample_rate: built.rate,
        keys_mapped: built.keys.keys().map(|k| k.code_name().to_owned()).collect(),
        keys_with_release: built.keys.values().filter(|(_, r)| !r.is_empty()).count(),
        split: built.split,
        skipped: notes.skipped,
        missing_files: notes.missing.into_iter().collect(),
        unreadable_files: built.unreadable,
        warnings,
        sounds_written,
        pack_warnings: checked
            .warnings
            .iter()
            .map(|p| format!("{}: {}", p.location, p.message))
            .collect(),
        replaced,
    })
}

/// The rate the written pack is test-loaded at (the loader's limits are defined at 48 kHz).
fn decode_check_rate() -> u32 {
    load::LIMIT_REFERENCE_RATE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_and_names() {
        assert_eq!(slug("CherryMX Black - ABS keycaps", 57), "cherrymx-black-abs-keycaps");
        assert_eq!(slug("  Zelda 3; Link to the Past ", 57), "zelda-3-link-to-the-past");
        assert_eq!(slug("Ünïcode ✓", 57), "n-code");
        assert_eq!(slug("日本語", 57), "");
        assert_eq!(slug("a".repeat(80).as_str(), 57).len(), 57);
        assert_eq!(clean("Synthetic multi pack \n", 64), "Synthetic multi pack");
        assert_eq!(clean(&"x".repeat(70), 64).len(), 64);
    }

    #[test]
    fn sniffing_goes_by_content() {
        assert_eq!(sniff(b"RIFF\0\0\0\0WAVEfmt ", "x.mp3"), Ok("wav"));
        assert_eq!(sniff(b"OggS\0\0", "noext"), Ok("ogg"));
        assert_eq!(sniff(b"ID3\x04", "a.wav"), Ok("mp3"));
        assert!(sniff(b"\0\0\0\x20ftypM4A ", "a.m4a").unwrap_err().contains("M4A"));
        assert!(sniff(b"fLaC", "a.flac").unwrap_err().contains("FLAC"));
        assert_eq!(sniff(b"????", "a.OGG"), Ok("ogg"));
        assert!(sniff(b"????", "a.m4a").is_err());
        assert!(sniff(b"????", "noext").is_err());
    }
}
