//! Turns pack parameters into a finished pack folder: plans every sample, renders it,
//! loudness-matches the packs, trims and writes 48 kHz 16-bit mono WAVs, `pack.json`, a
//! typed preview and `SOURCES.md` (provenance and the SHA-256 of every file).

use crate::analysis::{self, Metrics, Summary};
use crate::dsp::{FS, Rng, db_to_gain, fade_out, gain_to_db, peak, samples};
use crate::layout::{self, Voice};
use crate::params::{Action, Class, PackParams};
use crate::render::{Take, render};
use crate::sha256;
use hound::{SampleFormat, WavSpec, WavWriter};
use std::collections::BTreeMap;
use std::fs;
use std::io::Cursor;
use std::path::Path;
use taktak_core::audio::{DEFAULT_HUMANIZE, consistent_index};
use taktak_core::key::{Key, KeyGroup};
use taktak_core::pack::manifest::{FORMAT_VERSION, Manifest, SoundSet, VariationSpec};

pub const SOURCE: &str = "Synthesized by tools/synth-packs";
const AUTHOR: &str = "TakTak contributors";
const LICENSE: &str = "CC0-1.0";
const VERSION: &str = "1.0.0";
const PREVIEW: &str = "preview.wav";
/// The provenance record the bundled-pack rules require (docs/pack-format.md).
const SOURCES_MD: &str = "SOURCES.md";
const GENERATOR: &str = concat!("synth-packs ", env!("CARGO_PKG_VERSION"));
const REGENERATE: &str = "cargo run -p synth-packs --release -- --out packs";
const LICENSE_URL: &str = "https://creativecommons.org/publicdomain/zero/1.0/";
/// The preview is decoded like any other sample, so it must stay under the 2 s sample limit.
const PREVIEW_MAX_MS: f64 = 1950.0;

/// The loudest press of the loudest-crest pack peaks here; the others match its loudness.
const TARGET_PEAK_DBFS: f64 = -3.0;
/// Tails are cut once they stay below this level, then faded: well under audibility next to
/// a transient 57 dB louder, and it keeps every pack under 2 MB.
const TRIM_DBFS: f64 = -60.0;
const TRIM_MARGIN_MS: f64 = 3.0;
const MIN_LEN_MS: f64 = 40.0;
/// Group samples per action (space, enter, …).
const GROUP_VARIANTS: usize = 3;
/// The most frequent English letters get a second press take. By default the app keeps each
/// key on one of its takes; with random variants on, fast repeats of these letters vary.
const COMMON_LETTERS: [Key; 8] =
    [Key::KeyE, Key::KeyT, Key::KeyA, Key::KeyO, Key::KeyI, Key::KeyN, Key::KeyS, Key::KeyR];
/// Velocity spread of a take: per-key samples are a single take each, so they stay closer
/// to the key's own level and leave per-stroke variation to the runtime.
const KEY_TAKE: Take = Take { velocity_db: 0.6 };
const GROUP_TAKE: Take = Take { velocity_db: 1.0 };

/// One sample to synthesize.
#[derive(Clone, Debug)]
pub struct Job {
    pub path: String,
    pub class: Class,
    pub action: Action,
    pub voice: Voice,
    /// The key whose board position `voice` comes from; `None` for the average key.
    pub voiced_at: Option<Key>,
    pub take: Take,
    /// Row label in the analysis table.
    pub group: &'static str,
}

/// Every sample of a pack, plus the sound sets that reference them.
pub struct Plan {
    pub jobs: Vec<Job>,
    pub keys: BTreeMap<String, SoundSet>,
    pub groups: BTreeMap<String, SoundSet>,
}

fn file(stem: &str) -> String {
    format!("sounds/{stem}.wav")
}

fn action_name(a: Action) -> &'static str {
    match a {
        Action::Press => "press",
        Action::Release => "release",
    }
}

pub fn plan(p: &PackParams) -> Plan {
    let mut jobs = Vec::new();
    let mut keys = BTreeMap::new();
    let mut groups = BTreeMap::new();
    let mut job = |stem: String, class, action, (voice, voiced_at), take, group| {
        let path = file(&stem);
        jobs.push(Job { path: path.clone(), class, action, voice, voiced_at, take, group });
        path
    };

    // Per-key samples: every alphanumeric key, and both shifts (they have stabilizers).
    let per_key = Key::ALL
        .iter()
        .filter(|k| k.group() == KeyGroup::Alphanumeric)
        .map(|&k| (k, Class::Alnum, "alnum keys"))
        .chain([(Key::ShiftLeft, Class::Shift, "shift"), (Key::ShiftRight, Class::Shift, "shift")]);
    for (key, class, group) in per_key {
        let name = key.code_name();
        let Some(place) = layout::place(key) else { continue };
        let voice = (layout::voice(p.id, name, &place), Some(key));
        let mut set = SoundSet::default();
        set.press.push(job(format!("{name}-press"), class, Action::Press, voice, KEY_TAKE, group));
        if COMMON_LETTERS.contains(&key) {
            let stem = format!("{name}-press-2");
            set.press.push(job(stem, class, Action::Press, voice, KEY_TAKE, group));
        }
        let stem = format!("{name}-release");
        set.release.push(job(stem, class, Action::Release, voice, KEY_TAKE, group));
        keys.insert(name.to_owned(), set);
    }

    // Group samples. Each variant is a separate take; modifiers and "other" also move around
    // the board, since those groups cover keys in different places.
    let group_specs: [(KeyGroup, Class, &str, [Key; GROUP_VARIANTS]); 6] = [
        (KeyGroup::Alphanumeric, Class::Alnum, "alnum", [Key::KeyF, Key::KeyG, Key::KeyJ]),
        (KeyGroup::Space, Class::Space, "space", [Key::Space; 3]),
        (KeyGroup::Enter, Class::Enter, "enter", [Key::Enter; 3]),
        (KeyGroup::Backspace, Class::Backspace, "backspace", [Key::Backspace; 3]),
        (
            KeyGroup::Modifier,
            Class::Modifier,
            "modifiers",
            [Key::MetaLeft, Key::ControlLeft, Key::AltLeft],
        ),
        (KeyGroup::Other, Class::Other, "other", [Key::F5, Key::ArrowDown, Key::PageDown]),
    ];
    for (kg, class, stem, places) in group_specs {
        let label = if kg == KeyGroup::Alphanumeric { "alnum generic" } else { kg.name() };
        let mut set = SoundSet::default();
        for action in [Action::Press, Action::Release] {
            for (i, key) in places.iter().enumerate() {
                // The alphanumeric fallback is the average key, not any particular one.
                let voice = match (kg, layout::place(*key)) {
                    (KeyGroup::Alphanumeric, _) | (_, None) => (Voice::NEUTRAL, None),
                    (_, Some(pl)) => (layout::voice(p.id, key.code_name(), &pl), Some(*key)),
                };
                let path = job(
                    format!("{stem}-{}-{}", action_name(action), i + 1),
                    class,
                    action,
                    voice,
                    GROUP_TAKE,
                    label,
                );
                match action {
                    Action::Press => set.press.push(path),
                    Action::Release => set.release.push(path),
                }
            }
        }
        groups.insert(kg.name().to_owned(), set);
    }
    Plan { jobs, keys, groups }
}

pub struct RenderedPack {
    pub params: &'static PackParams,
    pub plan: Plan,
    pub audio: Vec<Vec<f64>>,
    pub gain_db: f64,
}

pub fn render_pack(p: &'static PackParams) -> RenderedPack {
    let plan = plan(p);
    let audio = plan
        .jobs
        .iter()
        .map(|j| {
            let mut rng = Rng::from_label(&format!("{}/{}", p.id, j.path));
            render(p, j.class, j.action, &j.voice, j.take, &mut rng)
        })
        .collect();
    RenderedPack { params: p, plan, audio, gain_db: 0.0 }
}

fn presses(r: &RenderedPack) -> impl Iterator<Item = (&Job, &Vec<f64>)> {
    r.plan.jobs.iter().zip(&r.audio).filter(|(j, _)| j.action == Action::Press)
}

/// Mean K-weighted energy of the per-key alphanumeric presses: what typing mostly sounds like.
fn typing_energy(r: &RenderedPack) -> f64 {
    let e: Vec<f64> = presses(r)
        .filter(|(j, _)| j.group == "alnum keys")
        .map(|(_, x)| analysis::k_energy(x))
        .collect();
    e.iter().sum::<f64>() / e.len().max(1) as f64
}

/// One gain per pack (never per file, so the press/release and key-class balance survives).
/// Packs are matched on typing loudness; the pack whose loudest press would clip first
/// (the highest crest factor) peaks at [`TARGET_PEAK_DBFS`], the others a little lower.
pub fn match_loudness(packs: &mut [RenderedPack]) {
    let at_target: Vec<(f64, f64)> = packs
        .iter()
        .map(|r| {
            let max_peak = presses(r).fold(0.0, |m, (_, x)| f64::max(m, peak(x)));
            let g = TARGET_PEAK_DBFS - gain_to_db(max_peak);
            (g, analysis::k_db(typing_energy(r)) + g)
        })
        .collect();
    let target = at_target.iter().map(|t| t.1).fold(f64::INFINITY, f64::min);
    for (r, (g, loud)) in packs.iter_mut().zip(at_target) {
        r.gain_db = g + target - loud;
    }
}

/// Cuts the inaudible end of a tail and fades out to exact zero.
fn trim_tail(x: &mut Vec<f64>, fade: usize) {
    let thr = db_to_gain(TRIM_DBFS);
    let last = x.iter().rposition(|s| s.abs() >= thr).unwrap_or(0);
    let end = (last + 1 + samples(TRIM_MARGIN_MS)).max(samples(MIN_LEN_MS)).min(x.len());
    x.truncate(end);
    fade_out(x, fade);
}

fn quantize(x: &[f64]) -> Vec<i16> {
    x.iter().map(|s| (s * 32767.0).round().clamp(-32768.0, 32767.0) as i16).collect()
}

fn dequantize(x: &[i16]) -> Vec<f64> {
    x.iter().map(|&s| f64::from(s) / 32767.0).collect()
}

pub fn wav_bytes(x: &[i16]) -> Result<Vec<u8>, String> {
    let spec = WavSpec {
        channels: 1,
        sample_rate: FS as u32,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut cur = Cursor::new(Vec::with_capacity(44 + 2 * x.len()));
    let mut w = WavWriter::new(&mut cur, spec).map_err(|e| e.to_string())?;
    for &s in x {
        w.write_sample(s).map_err(|e| e.to_string())?;
    }
    w.finalize().map_err(|e| e.to_string())?;
    Ok(cur.into_inner())
}

/// The preview phrase, typed with a made-up but human rhythm.
const PHRASE: [Key; 14] = [
    Key::KeyT,
    Key::KeyA,
    Key::KeyK,
    Key::KeyT,
    Key::KeyA,
    Key::KeyK,
    Key::Space,
    Key::KeyI,
    Key::KeyS,
    Key::Space,
    Key::KeyF,
    Key::KeyU,
    Key::KeyN,
    Key::Enter,
];

/// The files a key plays, following the pack's own key → group chain.
fn sound_set(plan: &Plan, key: Key) -> Option<&SoundSet> {
    plan.keys.get(key.code_name()).or_else(|| plan.groups.get(key.group().name()))
}

/// Linear-interpolation resampling by `ratio` (> 1 raises the pitch), like the runtime mixer.
fn resample(x: &[f64], ratio: f64) -> Vec<f64> {
    let n = ((x.len() - 1) as f64 / ratio).floor() as usize + 1;
    (0..n)
        .map(|i| {
            let pos = i as f64 * ratio;
            let k = pos as usize;
            let frac = pos - k as f64;
            x[k] * (1.0 - frac) + x.get(k + 1).copied().unwrap_or(0.0) * frac
        })
        .collect()
}

/// About 2 s of typing rendered from the pack's own final samples, the way the app plays it
/// by default: each key plays the take the app's consistent variant choice gives it
/// ([`consistent_index`]), with the pack's per-keystroke pitch and volume variation scaled by
/// the default humanize amount ([`DEFAULT_HUMANIZE`]).
fn render_preview(r: &RenderedPack, finals: &BTreeMap<&str, Vec<f64>>) -> Vec<f64> {
    let p = r.params;
    let humanize = f64::from(DEFAULT_HUMANIZE);
    let (pitch_range, volume_range) =
        (f64::from(p.variation_pitch) * humanize, f64::from(p.variation_volume) * humanize);
    let mut rng = Rng::from_label(&format!("{}/preview", p.id));
    let mut events: Vec<(f64, &[f64])> = Vec::new();
    let mut t = 0.0;
    for (i, &key) in PHRASE.iter().enumerate() {
        let Some(set) = sound_set(&r.plan, key) else { continue };
        let press = &set.press[consistent_index(key, set.press.len())];
        let release = &set.release[consistent_index(key, set.release.len())];
        let dwell = match key {
            Key::Space => rng.range(80.0, 115.0),
            Key::Enter => rng.range(90.0, 120.0),
            _ => rng.range(60.0, 110.0),
        };
        events.push((t, &finals[press.as_str()]));
        events.push((t + dwell, &finals[release.as_str()]));
        let next = PHRASE.get(i + 1).copied();
        t += if key == Key::Space || next == Some(Key::Space) {
            rng.range(130.0, 190.0)
        } else if next == Some(Key::Enter) {
            rng.range(160.0, 200.0)
        } else {
            rng.range(80.0, 145.0)
        };
    }
    // Tighten the rhythm just enough that the last tail, at the lowest runtime pitch (the
    // longest it can get), ends by the limit: end(k) = max(at·k + len) is linear per event.
    let slowest = 1.0 - pitch_range;
    let squeeze = events
        .iter()
        .filter(|(at, _)| *at > 0.0)
        .map(|(at, x)| (PREVIEW_MAX_MS - x.len() as f64 * 1e3 / FS / slowest) / at)
        .fold(1.0, f64::min);
    let voiced: Vec<(usize, Vec<f64>)> = events
        .into_iter()
        .map(|(at, x)| {
            let pitch = 1.0 + rng.range(-1.0, 1.0) * pitch_range;
            let vol = 1.0 + rng.range(-1.0, 1.0) * volume_range;
            (samples(at * squeeze), resample(x, pitch).into_iter().map(|s| s * vol).collect())
        })
        .collect();
    let len = voiced.iter().map(|(at, x)| at + x.len()).max().unwrap_or(0);
    let mut out = vec![0.0; len];
    for (at, x) in voiced {
        crate::dsp::mix(&mut out[at..], &x, 1.0);
    }
    // Overlapping tails can add up; keep 1 dB of headroom.
    let limit = db_to_gain(-1.0);
    let pk = peak(&out);
    if pk > limit {
        out.iter_mut().for_each(|s| *s *= limit / pk);
    }
    fade_out(&mut out, samples(2.0));
    out
}

/// What was written, for the analysis table.
pub struct PackReport {
    pub id: &'static str,
    pub name: &'static str,
    pub files: usize,
    pub bytes: u64,
    pub gain_db: f64,
    pub preview_ms: f64,
    pub rows: Vec<(&'static str, &'static str, Summary)>,
    pub typing: Summary,
    pub typing_release: Summary,
}

pub fn manifest(r: &RenderedPack) -> Manifest {
    let p = r.params;
    Manifest {
        format: FORMAT_VERSION,
        id: p.id.to_owned(),
        name: p.name.to_owned(),
        version: Some(VERSION.to_owned()),
        author: AUTHOR.to_owned(),
        license: LICENSE.to_owned(),
        description: Some(p.description.to_owned()),
        source: Some(SOURCE.to_owned()),
        attribution: None,
        preview: Some(PREVIEW.to_owned()),
        volume: None,
        trim_silence: Some(true),
        variation: Some(VariationSpec {
            pitch: Some(p.variation_pitch),
            volume: Some(p.variation_volume),
        }),
        groups: r.plan.groups.clone(),
        keys: r.plan.keys.clone(),
        extra: BTreeMap::new(),
    }
}

/// A file as written to the pack folder.
struct Shipped {
    path: String,
    bytes: usize,
    sha256: String,
}

fn class_label(c: Class) -> &'static str {
    match c {
        Class::Alnum => "alphanumeric key",
        Class::Space => "space bar (stabilized)",
        Class::Enter => "Enter (stabilized)",
        Class::Backspace => "Backspace (stabilized)",
        Class::Modifier => "modifier key",
        Class::Shift => "Shift (stabilized)",
        Class::Other => "function, arrow or navigation key",
    }
}

/// `SOURCES.md`: the provenance record of docs/pack-format.md ("Bundled packs"). Everything
/// in it is derived from the parameters and the files just written, so it is as
/// reproducible as they are: no dates, paths or machine details.
fn sources_md(r: &RenderedPack, shipped: &[Shipped]) -> String {
    let p = r.params;
    let id = p.id;
    let others: Vec<String> = crate::params::PACKS.iter().map(|q| format!("`{}`", q.id)).collect();

    // What each sample plays for, from the manifest's own sound sets.
    let mut roles: BTreeMap<&str, String> = BTreeMap::new();
    for (kind, sets) in [("keys", &r.plan.keys), ("groups", &r.plan.groups)] {
        for (name, set) in sets {
            for (action, paths) in [("press", &set.press), ("release", &set.release)] {
                for (i, path) in paths.iter().enumerate() {
                    let take = if paths.len() > 1 {
                        format!(", take {} of {}", i + 1, paths.len())
                    } else {
                        String::new()
                    };
                    roles.insert(path, format!("`{kind}.{name}` {action}{take}"));
                }
            }
        }
    }
    let jobs: BTreeMap<&str, &Job> = r.plan.jobs.iter().map(|j| (j.path.as_str(), j)).collect();

    let mut md = vec![
        "<!-- generated by tools/synth-packs: change the generator, not this file -->".to_owned(),
        format!("# {}: sources", p.name),
        String::new(),
        "Every file in this pack was procedurally synthesized by TakTak's own generator, \
         `tools/synth-packs`. No recordings, samples, impulse responses or other third-party \
         audio were used at any stage: each sound is computed from a physical model (contact \
         pulses, damped modal resonators, filtered noise) whose parameters are in \
         `tools/synth-packs/src/params.rs`. `tools/synth-packs/README.md` describes the model."
            .to_owned(),
        String::new(),
        "## License".to_owned(),
        String::new(),
        format!("- Author: {AUTHOR}."),
        format!(
            "- License: `{LICENSE}` ({LICENSE_URL}). The TakTak contributors dedicate every file \
             of this pack to the public domain under CC0 1.0 Universal."
        ),
        "- Proof: the pack is the output of `tools/synth-packs`, written for TakTak and part of \
         its repository (MIT). With no third-party input there is no upstream license or credit \
         to carry. The dedication is stated here and in `pack.json` (`license`). CC0 allows \
         copying, modifying and distributing, including commercially, without permission or \
         credit, so the pack needs no `LICENSE.txt`."
            .to_owned(),
        String::new(),
        "## Generator and seeds".to_owned(),
        String::new(),
        format!(
            "- Generator: {GENERATOR} (the TakTak workspace version). Pack version {VERSION}, \
             pack format {FORMAT_VERSION}."
        ),
        "- Every random choice comes from a SplitMix64 generator seeded with the 64-bit FNV-1a \
         hash of a fixed label:"
            .to_owned(),
        format!(
            "  - each sample: `{id}/<file>`, for example `{id}/sounds/KeyA-press.wav` (velocity, \
             contact time, mode frequencies, gains and decays, bounces, noise);"
        ),
        format!(
            "  - each key's voicing: `{id}/voice/<key>`, for example `{id}/voice/KeyA` (small \
             fixed pitch and level offsets on top of the key's board position, row and finger);"
        ),
        format!("  - the preview: `{id}/preview` (typing rhythm, per-keystroke pitch and volume)."),
        "- Nothing depends on the clock, the machine or the order of work, so a re-run writes \
         byte-identical files on the same platform. Another OS or toolchain could differ in the \
         last bit if its `libm` rounds differently: the checksums below would change, the sound \
         would not audibly."
            .to_owned(),
        String::new(),
        "## Regenerating".to_owned(),
        String::new(),
        "From the repository root:".to_owned(),
        String::new(),
        "```".to_owned(),
        REGENERATE.to_owned(),
        "```".to_owned(),
        String::new(),
        format!(
            "This rewrites all the synthesized packs ({}) together, because they are \
             loudness-matched against each other, and writes this file last. The generator only \
             replaces folders whose `pack.json` it wrote, and removes its own previous files \
             first, so nothing stale is left behind.",
            others.join(", ")
        ),
        String::new(),
        "## How each file was made".to_owned(),
        String::new(),
        format!(
            "- `sounds/*.wav`: each sample is rendered on its own at 48 kHz in 64-bit floating \
             point by `tools/synth-packs/src/render.rs`. The impact is at t = 0, so there is no \
             leading silence: every sample sounds within 0.5 ms. The model ends with a \
             2nd-order {} Hz high-pass and {} Hz low-pass, the event's level and a {} ms \
             raised-cosine fade. Then, for this pack: one gain of {:+.2} dB for every file (never \
             per file; it matches the packs' typing loudness), the tail cut {} ms after the last \
             sample at or above {} dBFS (keeping at least {} ms), the same {} ms raised-cosine \
             fade-out ending on an exact zero, and rounding to 16-bit PCM, mono. No resampling, \
             no per-file normalization, no leading trim.",
            p.hpf, p.lpf, p.fade_ms, r.gain_db, TRIM_MARGIN_MS, TRIM_DBFS, MIN_LEN_MS, p.fade_ms
        ),
        format!(
            "- `{PREVIEW}`: the pack's own final 16-bit samples typing \"taktak is fun\" and Enter \
             with seeded human timing, played the way the app plays them by default. Each key \
             plays the one take the app gives it (the letters repeated in \"taktak\" replay the \
             same files), and each keystroke is resampled (linear interpolation) for the pitch \
             variation ({}) and scaled for the volume variation ({}) from `pack.json`, both \
             times the app's default humanize amount ({}). The keystrokes are mixed, limited to \
             a -1 dBFS peak where tails overlap, faded out over 2 ms and rounded to 16-bit PCM, \
             mono; at most {} s.",
            p.variation_pitch,
            p.variation_volume,
            DEFAULT_HUMANIZE,
            PREVIEW_MAX_MS / 1e3
        ),
        "- `pack.json`: written by the generator from the same plan of samples.".to_owned(),
        String::new(),
        "## Files".to_owned(),
        String::new(),
        format!(
            "{} samples, `{PREVIEW}` and `pack.json`. This file is not listed: it cannot hold \
             its own checksum. \"Voiced at\" is the board position the sample was voiced for; \
             the generic alphanumeric samples use the average key.",
            r.plan.jobs.len()
        ),
        String::new(),
        "| File | Plays for | Synthesized as | Voiced at | Bytes | SHA-256 |".to_owned(),
        "|---|---|---|---|---:|---|".to_owned(),
    ];
    for f in shipped.iter().filter(|f| f.path != SOURCES_MD) {
        let (role, what, at) = match jobs.get(f.path.as_str()) {
            Some(j) => (
                roles.get(f.path.as_str()).cloned().unwrap_or_default(),
                format!("{}, {}", class_label(j.class), action_name(j.action)),
                j.voiced_at.map_or("average key".to_owned(), |k| format!("`{}`", k.code_name())),
            ),
            None if f.path == PREVIEW => {
                ("pack preview".to_owned(), "mix of the samples above".to_owned(), "—".to_owned())
            }
            None => ("manifest".to_owned(), "—".to_owned(), "—".to_owned()),
        };
        md.push(format!(
            "| `{}` | {role} | {what} | {at} | {} | `{}` |",
            f.path, f.bytes, f.sha256
        ));
    }
    md.push(String::new());
    md.join("\n")
}

/// Makes `dir` ready for a fresh copy of the pack. An existing folder is only cleared if it
/// is a previous output of this generator; anything else is left alone.
fn prepare_dir(dir: &Path) -> Result<(), String> {
    let err = |e: std::io::Error| format!("{}: {e}", dir.display());
    if dir.exists() {
        let ours = fs::read(dir.join("pack.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<Manifest>(&b).ok())
            .is_some_and(|m| m.source.as_deref() == Some(SOURCE));
        let empty = fs::read_dir(dir).map_err(err)?.next().is_none();
        if !ours && !empty {
            return Err(format!(
                "{}: exists and is not a pack generated by synth-packs; refusing to overwrite",
                dir.display()
            ));
        }
        // Remove only what this generator writes, so renamed samples leave no stale files.
        let sounds = dir.join("sounds");
        if sounds.is_dir() {
            for entry in fs::read_dir(&sounds).map_err(err)? {
                let path = entry.map_err(err)?.path();
                if path.extension().is_some_and(|e| e == "wav") {
                    fs::remove_file(&path).map_err(err)?;
                }
            }
        }
        for f in ["pack.json", PREVIEW, SOURCES_MD] {
            let path = dir.join(f);
            if path.exists() {
                fs::remove_file(&path).map_err(err)?;
            }
        }
    }
    fs::create_dir_all(dir.join("sounds")).map_err(err)
}

/// Normalizes, trims and writes one rendered pack to `out/<id>/`.
pub fn write_pack(out: &Path, r: &RenderedPack) -> Result<PackReport, String> {
    let p = r.params;
    let dir = out.join(p.id);
    prepare_dir(&dir)?;
    let gain = db_to_gain(r.gain_db);
    let fade = samples(p.fade_ms);

    let mut finals: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
    let mut metrics: Vec<Metrics> = Vec::new();
    let mut shipped: Vec<Shipped> = Vec::new();
    let write = |shipped: &mut Vec<Shipped>, rel: &str, data: &[u8]| {
        let path = dir.join(rel);
        fs::write(&path, data).map_err(|e| format!("{}: {e}", path.display()))?;
        let sha256 = sha256::hex(data);
        shipped.push(Shipped { path: rel.to_owned(), bytes: data.len(), sha256 });
        Ok::<(), String>(())
    };
    for (job, x) in r.plan.jobs.iter().zip(&r.audio) {
        let mut y: Vec<f64> = x.iter().map(|s| s * gain).collect();
        trim_tail(&mut y, fade);
        let pcm = quantize(&y);
        write(&mut shipped, &job.path, &wav_bytes(&pcm)?)?;
        let y = dequantize(&pcm);
        metrics.push(analysis::measure(&y));
        finals.insert(job.path.as_str(), y);
    }

    let preview = quantize(&render_preview(r, &finals));
    write(&mut shipped, PREVIEW, &wav_bytes(&preview)?)?;

    let mut json = serde_json::to_string_pretty(&manifest(r)).map_err(|e| e.to_string())?;
    json.push('\n');
    write(&mut shipped, "pack.json", json.as_bytes())?;

    // Last, since it lists the checksums of everything above.
    let sources = sources_md(r, &shipped);
    write(&mut shipped, SOURCES_MD, sources.as_bytes())?;
    let bytes = shipped.iter().map(|f| f.bytes as u64).sum();

    let mut rows = Vec::new();
    let mut labels: Vec<&'static str> = Vec::new();
    for j in &r.plan.jobs {
        if !labels.contains(&j.group) {
            labels.push(j.group);
        }
    }
    let select = |label: &str, action: Action| -> Vec<Metrics> {
        r.plan
            .jobs
            .iter()
            .zip(&metrics)
            .filter(|(j, _)| j.group == label && j.action == action)
            .map(|(_, m)| *m)
            .collect()
    };
    for label in labels {
        for action in [Action::Press, Action::Release] {
            rows.push((label, action_name(action), analysis::summarize(&select(label, action))));
        }
    }
    Ok(PackReport {
        id: p.id,
        name: p.name,
        files: shipped.len(),
        bytes,
        gain_db: r.gain_db,
        preview_ms: preview.len() as f64 * 1e3 / FS,
        rows,
        typing: analysis::summarize(&select("alnum keys", Action::Press)),
        typing_release: analysis::summarize(&select("alnum keys", Action::Release)),
    })
}

/// Renders all packs, matches their loudness and writes them under `out`.
pub fn generate(out: &Path, packs: &[&'static PackParams]) -> Result<Vec<PackReport>, String> {
    let mut rendered: Vec<RenderedPack> = packs.iter().map(|p| render_pack(p)).collect();
    match_loudness(&mut rendered);
    rendered.iter().map(|r| write_pack(out, r)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::PACKS;
    use taktak_core::pack::manifest::GROUP_NAMES;

    fn read_wav(path: &Path) -> (WavSpec, Vec<i16>) {
        let mut r = hound::WavReader::open(path).unwrap();
        let spec = r.spec();
        (spec, r.samples::<i16>().map(|s| s.unwrap()).collect())
    }

    fn check_path_syntax(path: &str) {
        assert!(path.starts_with("sounds/") && path.ends_with(".wav"), "{path}");
        assert!(!path.contains('\\') && !path.contains("//") && !path.contains(".."), "{path}");
    }

    #[test]
    fn generated_packs_are_complete_valid_and_small() {
        let tmp = tempfile::tempdir().unwrap();
        let reports = generate(tmp.path(), &PACKS).unwrap();
        assert_eq!(reports.len(), 3);
        for (p, report) in PACKS.iter().zip(&reports) {
            let dir = tmp.path().join(p.id);
            let m: Manifest =
                serde_json::from_slice(&fs::read(dir.join("pack.json")).unwrap()).unwrap();
            assert_eq!(m.format, FORMAT_VERSION);
            assert_eq!(m.id, p.id);
            assert_eq!(m.license, "CC0-1.0");
            assert_eq!(m.trim_silence, Some(true));
            let v = m.variation.unwrap();
            assert!(v.pitch.unwrap() <= 0.10 && v.volume.unwrap() <= 0.50);

            for &k in Key::ALL.iter().filter(|k| k.group() == KeyGroup::Alphanumeric) {
                let set = m.keys.get(k.code_name()).expect("every alphanumeric key has samples");
                assert!(!set.press.is_empty() && !set.release.is_empty());
            }
            for g in GROUP_NAMES {
                let set = m.groups.get(g).expect("every group has samples");
                assert_eq!(set.press.len(), GROUP_VARIANTS);
                assert_eq!(set.release.len(), GROUP_VARIANTS);
            }
            // Only names the loader accepts: real key names and the six spec groups.
            for name in m.keys.keys() {
                assert!(name.parse::<Key>().is_ok(), "unknown key name {name}");
            }
            assert!(m.groups.keys().all(|g| GROUP_NAMES.contains(&g.as_str())));

            let mut total = 0u64;
            let mut seen = std::collections::BTreeSet::new();
            for set in m.keys.values().chain(m.groups.values()) {
                for path in set.press.iter().chain(&set.release) {
                    check_path_syntax(path);
                    let (spec, pcm) = read_wav(&dir.join(path));
                    assert_eq!(
                        (spec.channels, spec.sample_rate, spec.bits_per_sample),
                        (1, 48_000, 16)
                    );
                    assert_eq!(*pcm.last().unwrap(), 0, "{path} must end at zero");
                    assert!(pcm.len() <= samples(2000.0), "{path} is longer than 2 s");
                    // Onset within 0.5 ms: the loader trims nothing.
                    let onset = pcm.iter().position(|s| (*s as f64 / 32767.0).abs() >= 0.00316);
                    assert!(onset.unwrap() <= samples(0.5), "{path} onset {onset:?}");
                    if seen.insert(path.clone()) {
                        total += fs::metadata(dir.join(path)).unwrap().len();
                    }
                }
            }
            let (_, preview) = read_wav(&dir.join(m.preview.unwrap()));
            let secs = preview.len() as f64 / FS;
            assert!((1.6..=PREVIEW_MAX_MS / 1e3).contains(&secs), "preview is {secs} s");
            total += fs::metadata(dir.join(PREVIEW)).unwrap().len();
            total += fs::metadata(dir.join("pack.json")).unwrap().len();
            total += fs::metadata(dir.join(SOURCES_MD)).unwrap().len();
            assert_eq!(total, report.bytes, "no unreferenced files");
            assert_eq!(report.files, walk(&dir).len());
            check_sources(&dir, seen.len());
            assert!(total < 2_000_000, "{} is {total} bytes", p.id);
            let peak_db = report.rows.iter().map(|r| r.2.peak_db).fold(f64::MIN, f64::max);
            assert!(peak_db <= TARGET_PEAK_DBFS + 0.1, "{} peaks at {peak_db}", p.id);
        }
        // Loudness-matched within 0.5 dB on typing.
        let k: Vec<f64> = reports.iter().map(|r| r.typing.k_db).collect();
        let spread =
            k.iter().fold(f64::MIN, |a, &b| a.max(b)) - k.iter().fold(f64::MAX, |a, &b| a.min(b));
        assert!(spread < 0.5, "typing loudness {k:?}");
    }

    /// `SOURCES.md` states the license and how to regenerate, and lists every other file in
    /// the folder with its size and correct SHA-256.
    fn check_sources(dir: &Path, samples: usize) {
        let md = fs::read_to_string(dir.join(SOURCES_MD)).unwrap();
        for needle in ["procedurally synthesized", "No recordings", "`CC0-1.0`", REGENERATE] {
            assert!(md.contains(needle), "SOURCES.md does not mention {needle:?}");
        }
        let files = walk(dir);
        let rows: Vec<&str> = md.lines().filter(|l| l.starts_with("| `")).collect();
        assert_eq!(rows.len(), files.len() - 1, "one row per file except SOURCES.md");
        assert_eq!(rows.len(), samples + 2, "every sample, the preview and pack.json");
        for (rel, data) in files.iter().filter(|(rel, _)| rel != SOURCES_MD) {
            let row = rows
                .iter()
                .find(|r| r.starts_with(&format!("| `{rel}` |")))
                .unwrap_or_else(|| panic!("SOURCES.md does not list {rel}"));
            let cells: Vec<&str> = row.split(" | ").collect();
            assert_eq!(cells[cells.len() - 2], data.len().to_string(), "{rel} size");
            assert_eq!(cells[cells.len() - 1], format!("`{}` |", sha256::hex(data)), "{rel}");
        }
    }

    #[test]
    fn regenerating_is_byte_identical_and_replaces_stale_files() {
        let tmp = tempfile::tempdir().unwrap();
        let p = &crate::params::CRISP_CLACK;
        let mut r = render_pack(p);
        r.gain_db = -6.0;
        write_pack(tmp.path(), &r).unwrap();
        let dir = tmp.path().join(p.id);
        let first = walk(&dir);
        assert!(first.iter().any(|(rel, _)| rel == SOURCES_MD));
        fs::write(dir.join("sounds/stale.wav"), b"x").unwrap();
        fs::write(dir.join(SOURCES_MD), b"edited by hand").unwrap();
        let mut r2 = render_pack(p);
        r2.gain_db = -6.0;
        write_pack(tmp.path(), &r2).unwrap();
        assert_eq!(first, walk(&dir));
        // Nothing in the output depends on where it is written.
        let elsewhere = tempfile::tempdir().unwrap();
        write_pack(elsewhere.path(), &r2).unwrap();
        assert_eq!(first, walk(&elsewhere.path().join(p.id)));
    }

    #[test]
    fn refuses_to_overwrite_a_foreign_folder() {
        let tmp = tempfile::tempdir().unwrap();
        for (name, file) in [("crisp-clack", "notes.txt"), ("deep-thock", SOURCES_MD)] {
            let dir = tmp.path().join(name);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join(file), b"mine").unwrap();
            assert!(prepare_dir(&dir).is_err(), "{name}/{file}");
            assert_eq!(fs::read(dir.join(file)).unwrap(), b"mine");
        }
    }

    /// Every file of a pack folder as (path relative to `dir`, contents), sorted.
    fn walk(dir: &Path) -> Vec<(String, Vec<u8>)> {
        let mut out = Vec::new();
        for (prefix, sub) in [("", dir.to_path_buf()), ("sounds/", dir.join("sounds"))] {
            for e in fs::read_dir(&sub).unwrap() {
                let path = e.unwrap().path();
                if path.is_file() {
                    let name = path.file_name().unwrap().to_string_lossy();
                    out.push((format!("{prefix}{name}"), fs::read(&path).unwrap()));
                }
            }
        }
        out.sort();
        out
    }
}
