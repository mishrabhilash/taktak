//! The Mechvibes importer end to end: synthetic Mechvibes, Mechvibes v2, Mechvibes++ and
//! MechvibesDX packs (folders and zips) in, valid TakTak packs out.
//!
//! All audio is synthetic. `fixtures/mv-sprite-3keys-44100.ogg` (1.3 s, three keystrokes at
//! 0.1, 0.5 and 0.9 s, each a press click + bottom-out and a release 120 ms later, over faint
//! noise) was generated with FFmpeg's `aevalsrc` and its Vorbis encoder; no recordings or
//! third-party packs are involved.

mod common;

use common::{click, fixture, wav};
use std::fs::{self, File};
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use taktak_core::input::KeyAction;
use taktak_core::key::Key;
use taktak_core::pack::import::{
    ImportError, ImportOptions, ImportReport, SourceFormat, import_mechvibes,
};
use taktak_core::pack::manifest::{self, Manifest};
use taktak_core::pack::{PackOrigin, decode, load};

const SPRITE_RATE: u32 = 44_100;
const LEAD_MS: f64 = 30.0;
const RELEASE_MS: f64 = 110.0;
const SLICE_MS: f64 = 200.0;

/// Deterministic noise in [-1, 1].
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

fn ms(rate: u32, ms: f64) -> usize {
    (ms * f64::from(rate) / 1000.0).round() as usize
}

fn add_burst(
    out: &mut [f32],
    rate: u32,
    at_ms: f64,
    len_ms: f64,
    amp: f32,
    tau_ms: f32,
    seed: u32,
) {
    let mut noise = Noise(seed.wrapping_mul(2_654_435_761).max(1));
    let start = ms(rate, at_ms);
    for i in 0..ms(rate, len_ms) {
        if let Some(o) = out.get_mut(start + i) {
            let t = i as f32 / rate as f32 * 1000.0;
            *o += amp * noise.next() * (-t / tau_ms).exp();
        }
    }
}

/// A sprite with `count` whole keystrokes, slot `i` starting at `i * 400 ms`: 30 ms of room
/// tone, then a press (click + bottom-out) and a release 110 ms after the press.
fn sprite_samples(count: usize) -> Vec<f32> {
    let total = ms(SPRITE_RATE, 400.0 * count as f64 + 200.0);
    let mut out = vec![0.0f32; total];
    let mut noise = Noise(99);
    for s in &mut out {
        *s = 0.0008 * noise.next();
    }
    for i in 0..count {
        let t0 = 400.0 * i as f64 + LEAD_MS;
        let seed = i as u32 * 7 + 1;
        add_burst(&mut out, SPRITE_RATE, t0, 40.0, 0.6, 6.0, seed);
        add_burst(&mut out, SPRITE_RATE, t0 + 12.0, 40.0, 0.35, 8.0, seed + 1);
        add_burst(&mut out, SPRITE_RATE, t0 + RELEASE_MS, 50.0, 0.3, 7.0, seed + 2);
    }
    out
}

fn wav_bytes(samples: &[f32], rate: u32) -> Vec<u8> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut cursor = Cursor::new(Vec::new());
    let mut w = hound::WavWriter::new(&mut cursor, spec).unwrap();
    for s in samples {
        w.write_sample((s.clamp(-1.0, 1.0) * 32767.0).round() as i16).unwrap();
    }
    w.finalize().unwrap();
    cursor.into_inner()
}

fn sprite_wav(count: usize) -> Vec<u8> {
    wav_bytes(&sprite_samples(count), SPRITE_RATE)
}

/// `[start_ms, length_ms]` of slot `i` in [`sprite_samples`], lead-in included.
fn slot(i: usize) -> String {
    format!("[{}, {SLICE_MS}]", 400 * i)
}

fn write_pack(dir: &Path, config: &str, files: &[(&str, Vec<u8>)]) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join("config.json"), config).unwrap();
    for (rel, bytes) in files {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    dir.to_path_buf()
}

fn write_zip(path: &Path, entries: &[(&str, Vec<u8>)]) -> PathBuf {
    let mut w = zip::ZipWriter::new(File::create(path).unwrap());
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in entries {
        w.start_file(*name, options).unwrap();
        w.write_all(bytes).unwrap();
    }
    w.finish().unwrap();
    path.to_path_buf()
}

struct Env {
    tmp: tempfile::TempDir,
}

impl Env {
    fn new() -> Env {
        Env { tmp: tempfile::tempdir().unwrap() }
    }

    fn src(&self, name: &str) -> PathBuf {
        self.tmp.path().join("src").join(name)
    }

    fn dest(&self) -> PathBuf {
        self.tmp.path().join("packs")
    }

    fn import(&self, src: &Path) -> Result<ImportReport, ImportError> {
        import_mechvibes(src, &self.dest(), ImportOptions::default())
    }

    fn import_with(&self, src: &Path, opts: ImportOptions) -> Result<ImportReport, ImportError> {
        import_mechvibes(src, &self.dest(), opts)
    }
}

/// Checks the written pack the way the app would and returns its manifest. `extra_warnings`
/// are the loader warnings allowed besides the personal-license one.
fn check(report: &ImportReport, extra_warnings: &[&str]) -> Manifest {
    let (info, warnings) =
        load::inspect(&report.path, PackOrigin::User, false).unwrap_or_else(|e| panic!("{e}"));
    let mut locations: Vec<&str> = warnings.iter().map(|p| p.location.as_str()).collect();
    locations.sort_unstable();
    let mut expected = vec!["license"];
    expected.extend_from_slice(extra_warnings);
    expected.sort_unstable();
    assert_eq!(locations, expected, "{warnings:?}");
    assert_eq!(info.license, "LicenseRef-Personal");
    assert_eq!(info.id, report.id);
    let loaded =
        load::load(&report.path, PackOrigin::User, 48_000).unwrap_or_else(|e| panic!("{e}"));
    let preview = loaded.preview.expect("preview");
    assert!(preview.len() > 48_000, "preview is {} samples", preview.len());
    // Every key sounds on press.
    let m = load::read_manifest(&report.path).unwrap();
    for &key in Key::ALL {
        assert!(!manifest::resolve(&m, key, KeyAction::Down).is_empty());
    }
    assert!(report.path.ends_with(&report.id));
    m
}

fn decoded(report: &ImportReport, rel: &str) -> Vec<f32> {
    let bytes = fs::read(report.path.join(rel)).unwrap();
    decode::decode(bytes, "wav").unwrap().samples
}

fn key_files<'a>(m: &'a Manifest, key: &str, action: KeyAction) -> &'a [String] {
    manifest::resolve(m, key.parse().unwrap(), action)
}

fn v1_sprite_config(name: &str, sound: &str, defines: &str) -> String {
    format!(
        r#"{{"id": "custom-sound-pack-1", "name": "{name}", "key_define_type": "single",
            "includes_numpad": false, "sound": "{sound}", "defines": {{ {defines} }} }}"#
    )
}

#[test]
fn v1_sprite_pack_imports_with_release_split() {
    let env = Env::new();
    let defines = format!(
        r#""30": {}, "31": {}, "57": {}, "28": {}, "14": {}, "1": {}"#,
        slot(0),
        slot(1),
        slot(2),
        slot(3),
        slot(4),
        slot(5)
    );
    let src = write_pack(
        &env.src("my-sprite-pack"),
        &v1_sprite_config("My Sprite Pack", "sound.wav", &defines),
        &[("sound.wav", sprite_wav(6))],
    );
    let report = env.import(&src).unwrap();
    assert_eq!(report.id, "mv-my-sprite-pack");
    assert_eq!(report.format, SourceFormat::MechvibesV1Sprite);
    assert_eq!(report.sample_rate, SPRITE_RATE);
    assert_eq!(report.keys_mapped.len(), 6);
    assert_eq!(report.keys_with_release, 6);
    assert_eq!((report.split.eligible, report.split.detected), (6, 6));
    assert!(report.skipped.is_empty() && report.missing_files.is_empty());
    let m = check(&report, &[]);
    assert_eq!(m.author, "Unknown (imported from Mechvibes)");
    assert_eq!(m.source.as_deref(), Some("my-sprite-pack"));
    assert!(m.description.as_deref().unwrap().contains("personal use"));
    assert_eq!(m.preview.as_deref(), Some("preview.wav"));

    // Press starts at its onset (the 30 ms lead-in is gone) and holds no release transient.
    let press = decoded(&report, &key_files(&m, "KeyA", KeyAction::Down)[0]);
    let lead = decode::leading_silence(&press);
    assert!(lead < ms(SPRITE_RATE, 1.0), "lead {lead}");
    assert!(press.len() < ms(SPRITE_RATE, RELEASE_MS), "press {} samples", press.len());
    assert!(press.len() > ms(SPRITE_RATE, 60.0), "press {} samples", press.len());
    let release = decoded(&report, &key_files(&m, "KeyA", KeyAction::Up)[0]);
    assert!(decode::leading_silence(&release) < ms(SPRITE_RATE, 1.0));
    // Every sound is faded out.
    assert!(press.last().unwrap().abs() < 0.01 && release.last().unwrap().abs() < 0.01);
    // The preview is ~1.9 s.
    let preview = decoded(&report, "preview.wav");
    assert!((1.85..1.95).contains(&(preview.len() as f32 / SPRITE_RATE as f32)));
    // Nothing temporary is left behind.
    let names: Vec<String> = fs::read_dir(env.dest())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, ["mv-my-sprite-pack"]);
}

#[test]
fn release_split_can_be_turned_off() {
    let env = Env::new();
    let defines = format!(r#""30": {}, "57": {}"#, slot(0), slot(1));
    let src = write_pack(
        &env.src("p"),
        &v1_sprite_config("No Split", "sound.wav", &defines),
        &[("sound.wav", sprite_wav(2))],
    );
    let opts = ImportOptions { split_release: false, ..ImportOptions::default() };
    let report = env.import_with(&src, opts).unwrap();
    assert_eq!(report.keys_with_release, 0);
    assert_eq!(report.split.kept_whole, 2);
    let m = check(&report, &["release"]);
    // The press holds the whole keystroke.
    let press = decoded(&report, &key_files(&m, "KeyA", KeyAction::Down)[0]);
    assert!(press.len() > ms(SPRITE_RATE, RELEASE_MS + 20.0));
}

#[test]
fn single_transient_slices_are_kept_whole() {
    let env = Env::new();
    // Clicks without a release: no clear second transient, so nothing is split.
    let defines = r#""30": [0, 100], "31": [200, 100], "57": [400, 100]"#;
    let mut samples = vec![0.0f32; ms(48_000, 600.0)];
    for (i, at) in [0.0, 200.0, 400.0].into_iter().enumerate() {
        add_burst(&mut samples, 48_000, at, 60.0, 0.7, 6.0, i as u32 + 3);
    }
    let src = write_pack(
        &env.src("clicks"),
        &v1_sprite_config("Clicks", "s.wav", defines),
        &[("s.wav", wav_bytes(&samples, 48_000))],
    );
    let report = env.import(&src).unwrap();
    assert_eq!(report.split.kept_whole, 3);
    assert_eq!(report.keys_with_release, 0);
    assert!(report.warnings.iter().any(|w| w.contains("clear release transient")));
    check(&report, &["release"]);
}

#[test]
fn ogg_sprite_with_quirky_defines() {
    let env = Env::new();
    let defines = r#""30": [70, 300], "31": [470, 300], "57": [870, 300],
        "69": [100, 0], "70": [1250, 200], "71": [5000, 100], "58": null,
        "91,91,92": [70, 300], "57378": [70, 300], "99999": [70, 300], "28-up": [990, 100]"#;
    let src = write_pack(
        &env.src("ogg-sprite"),
        &v1_sprite_config("Ogg Sprite", "Sound.OGG", defines),
        &[("sound.ogg", fixture("mv-sprite-3keys-44100.ogg"))],
    );
    let report = env.import(&src).unwrap();
    assert_eq!(report.sample_rate, 44_100);
    let skipped: Vec<&str> = report.skipped.iter().map(|s| s.key.as_str()).collect();
    assert_eq!(skipped, ["91,91,92", "57378", "99999"]);
    assert!(report.skipped[1].reason.contains("MediaPlayPause is not a key TakTak supports"));
    assert!(report.skipped[2].reason.contains("unknown"));
    assert!(report.warnings.iter().any(|w| w.contains("zero-length")));
    assert!(report.warnings.iter().any(|w| w.contains("past the end")));
    // NumLock is zero-length, ScrollLock straddles the end of the file and holds only noise
    // (dropped as silent), Numpad7 starts past the end, CapsLock is null; Enter has a release
    // only.
    assert!(report.warnings.iter().any(|w| w.contains("silent")));
    let mut mapped = report.keys_mapped.clone();
    mapped.sort();
    assert_eq!(mapped, ["Enter", "KeyA", "KeyS", "Space"]);
    assert_eq!(report.split.detected, 3);
    let m = check(&report, &[]);
    assert_eq!(m.keys["Enter"].press, Vec::<String>::new());
    assert_eq!(m.keys["Enter"].release.len(), 1);
}

#[test]
fn hand_cut_up_slices_are_used_as_releases() {
    let env = Env::new();
    // Press [30, 100) and release [140, 240) cut by hand, as in Mechvibes' cherrymx-black-abs.
    let defines = r#""30": [25, 80], "30-up": [138, 90], "31": [400, 200]"#;
    let src = write_pack(
        &env.src("up"),
        &v1_sprite_config("Up Slices", "sound.wav", defines),
        &[("sound.wav", sprite_wav(2))],
    );
    let report = env.import(&src).unwrap();
    // Only KeyS (no -up slice) is split.
    assert_eq!(report.split.eligible, 1);
    assert_eq!(report.keys_with_release, 2);
    check(&report, &[]);
}

#[test]
fn v1_multi_pack_tolerates_community_quirks() {
    let env = Env::new();
    let config = r#"{
      "id": "custom-sound-pack-1203000000044", "name": "Quirky multi pack ",
      "key_define_type": "multiple", "sound": "sound.ogg", "m_author": "Someone",
      "description": "Sounds\nfrom somewhere", "tags": ["x"], "_zip": "custom-sound-pack.zip",
      "defines": {
        "30": "Key A.wav", "26": "[.wav", "58": "caps lock.wav", "14": "backspace.mp3",
        "28": "enter.ogg", "57": "sfx-blink", "1": "missing.wav", "2": "", "3": null,
        "15": "mouse.m4a", "16": "sub\\q.wav"
      }
    }"#;
    let mut m4a = b"\0\0\0\x20ftypM4A \0\0\0\0".to_vec();
    m4a.resize(64, 0);
    let src = write_pack(
        &env.src("quirky"),
        config,
        &[
            ("Key A.wav", click(2_000)),
            ("[.wav", click(2_100)),
            ("caps lock.wav", click(2_200)),
            ("BACKSPACE.mp3", fixture("tone-click-mono-44100.mp3")),
            ("enter.ogg", fixture("tone-click-stereo-44100.ogg")),
            ("sfx-blink", click(2_300)),
            ("mouse.m4a", m4a),
            ("sub/q.wav", click(2_400)),
        ],
    );
    let report = env.import(&src).unwrap();
    assert_eq!(report.format, SourceFormat::MechvibesV1Files);
    assert_eq!(report.missing_files, ["missing.wav"]);
    assert_eq!(report.unreadable_files.len(), 1);
    assert!(report.unreadable_files[0].1.contains("M4A"), "{:?}", report.unreadable_files);
    let mut mapped = report.keys_mapped.clone();
    mapped.sort();
    assert_eq!(mapped, ["Backspace", "BracketLeft", "CapsLock", "Enter", "KeyA", "KeyQ", "Space"]);
    // Mixed rates (48 kHz WAV, 44.1 kHz MP3/Ogg) are written at 48 kHz.
    assert_eq!(report.sample_rate, 48_000);
    let m = check(&report, &["release"]);
    assert_eq!(m.name, "Quirky multi pack");
    assert_eq!(m.author, "Someone");
    let description = m.description.clone().unwrap();
    assert!(description.contains("Original description: Sounds from somewhere"));
    assert_eq!(decoded(&report, &key_files(&m, "KeyA", KeyAction::Down)[0]).len(), 2_000);
    assert_eq!(decoded(&report, &key_files(&m, "Space", KeyAction::Down)[0]).len(), 2_300);
    assert_eq!(decoded(&report, &key_files(&m, "KeyQ", KeyAction::Down)[0]).len(), 2_400);
}

#[test]
fn windows_aliases_only_fill_gaps_and_duplicates_keep_the_last() {
    let env = Env::new();
    let config = r#"{"name": "Aliases", "key_define_type": "multi", "sound": "", "defines": {
        "61000": "alias-up.wav", "57416": "up.wav", "61003": "alias-left.wav",
        "30": "first.wav", "30": "second.wav" }}"#;
    let src = write_pack(
        &env.src("aliases"),
        config,
        &[
            ("alias-up.wav", click(1_000)),
            ("up.wav", click(1_100)),
            ("alias-left.wav", click(1_200)),
            ("first.wav", click(1_300)),
            ("second.wav", click(1_400)),
        ],
    );
    let report = env.import(&src).unwrap();
    assert!(report.warnings.iter().any(|w| w.contains("appears twice")));
    let m = check(&report, &["release"]);
    let len = |key: &str| decoded(&report, &key_files(&m, key, KeyAction::Down)[0]).len();
    assert_eq!(len("ArrowUp"), 1_100);
    assert_eq!(len("ArrowLeft"), 1_200);
    assert_eq!(len("KeyA"), 1_400);
}

#[test]
fn evdev_numbered_packs_are_recognized() {
    let env = Env::new();
    let config = r#"{"name": "Linux pack", "key_define_type": "multiple", "defines": {
        "30": "a.wav", "97": "rctrl.wav", "103": "evdev-up.wav", "57416": "up.wav",
        "105": "left.wav" }}"#;
    let src = write_pack(
        &env.src("linux"),
        config,
        &[
            ("a.wav", click(1_000)),
            ("rctrl.wav", click(1_100)),
            ("evdev-up.wav", click(1_200)),
            ("up.wav", click(1_300)),
            ("left.wav", click(1_400)),
        ],
    );
    let report = env.import(&src).unwrap();
    assert!(report.warnings.iter().any(|w| w.contains("evdev")));
    let m = check(&report, &["release"]);
    let len = |key: &str| decoded(&report, &key_files(&m, key, KeyAction::Down)[0]).len();
    assert_eq!(len("ControlRight"), 1_100);
    assert_eq!(len("ArrowUp"), 1_300, "the libuiohook code wins");
    assert_eq!(len("ArrowLeft"), 1_400);
    assert!(!report.keys_mapped.contains(&"F20".to_owned()));
}

#[test]
fn v2_pack_with_up_keys_ranges_and_fallbacks() {
    let env = Env::new();
    let config = r#"{"id": "traveler", "name": "Traveler", "key_define_type": "multi",
        "sound": "press/GENERIC_R{0-2}.wav", "soundup": "release/GENERIC.wav",
        "defines": {"14": "press/BACKSPACE.wav", "14-up": "release/BACKSPACE.wav",
                    "57": "press/SPACE.wav", "57-up": "release/SPACE.wav", "30": null},
        "version": 2}"#;
    let src = write_pack(
        &env.src("traveler"),
        config,
        &[
            ("press/GENERIC_R0.wav", click(1_000)),
            ("press/GENERIC_R1.wav", click(1_100)),
            ("press/GENERIC_R2.wav", click(1_200)),
            ("release/GENERIC.wav", click(900)),
            ("press/BACKSPACE.wav", click(1_300)),
            ("release/BACKSPACE.wav", click(800)),
            ("press/SPACE.wav", click(1_400)),
            ("release/SPACE.wav", click(700)),
        ],
    );
    let report = env.import(&src).unwrap();
    assert_eq!(report.format, SourceFormat::MechvibesV2);
    assert_eq!(report.keys_with_release, 2);
    let m = check(&report, &[]);
    assert_eq!(m.groups["alphanumeric"].press.len(), 3);
    assert_eq!(m.groups["alphanumeric"].release.len(), 1);
    // KeyA (null in v2) and every undefined key use the fallbacks.
    assert!(!m.keys.contains_key("KeyA"));
    assert_eq!(key_files(&m, "KeyZ", KeyAction::Down).len(), 3);
    let len = |key: &str, action| decoded(&report, &key_files(&m, key, action)[0]).len();
    assert_eq!(len("Backspace", KeyAction::Up), 800);
    assert_eq!(len("Space", KeyAction::Down), 1_400);
    assert_eq!(len("Enter", KeyAction::Up), 900);
}

#[test]
fn mechvibes_plus_plus_compat_codes() {
    let env = Env::new();
    let config = r#"{"name": "Compat", "key_define_type": "multi", "compatibility": true,
        "defines": {"30": "plain-a.wav", "030": "a.wav", "0030": "a-up.wav", "31": "s.wav"}}"#;
    let src = write_pack(
        &env.src("compat"),
        config,
        &[
            ("plain-a.wav", click(1_000)),
            ("a.wav", click(1_100)),
            ("a-up.wav", click(1_200)),
            ("s.wav", click(1_300)),
        ],
    );
    let report = env.import(&src).unwrap();
    assert_eq!(report.format, SourceFormat::MechvibesPlusPlus { sprite: false });
    let m = check(&report, &[]);
    let len = |key: &str, action| decoded(&report, &key_files(&m, key, action)[0]).len();
    assert_eq!(len("KeyA", KeyAction::Down), 1_100);
    assert_eq!(len("KeyA", KeyAction::Up), 1_200);
    assert_eq!(len("KeyS", KeyAction::Down), 1_300);
}

fn dx_config(defs: &str, extra: &str) -> String {
    format!(
        r#"{{"audio_file": "sound.wav", "config_version": "2", "definition_method": "single",
            "author": "Mechvibes", "name": "DX Pack", "id": "keyboad-x",
            "definitions": {{ {defs} }} {extra} }}"#
    )
}

/// A DX timing pair over slot `i`: the slice cut at its midpoint, as MechvibesDX does.
fn dx_pair(i: usize) -> String {
    let s = 400.0 * i as f64;
    let mid = s + SLICE_MS / 2.0;
    format!(r#"{{"timing": [[{s}, {mid}], [{mid}, {}]]}}"#, s + SLICE_MS)
}

#[test]
fn dx_pack_resplits_midpoint_pairs_and_maps_options() {
    let env = Env::new();
    let defs = format!(
        r#""KeyA": {}, "Space": {}, "Enter": {{"timing": [[1200, 1400]]}},
           "Backspace": {{"timing": [[1600, 1700], [1700, 1800], [1800, 1810]]}},
           "MouseLeft": {{"timing": [[0, 10]]}}, "KeyFoo": {}"#,
        dx_pair(0),
        dx_pair(1),
        dx_pair(0)
    );
    let extra = r#", "options": {"recommended_volume": 0.8, "random_pitch": true}"#;
    let src = write_pack(&env.src("dx"), &dx_config(&defs, extra), &[("sound.wav", sprite_wav(5))]);
    let report = env.import(&src).unwrap();
    assert_eq!(report.format, SourceFormat::MechvibesDx);
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].key, "KeyFoo");
    assert!(report.warnings.iter().any(|w| w.contains("mouse")));
    assert!(report.warnings.iter().any(|w| w.contains("3 timings")));
    assert_eq!((report.split.eligible, report.split.detected), (2, 2));
    let m = check(&report, &[]);
    assert_eq!(m.author, "Mechvibes");
    assert_eq!(m.volume, Some(0.8));
    assert_eq!(m.variation.unwrap().pitch, Some(0.10));
    // The detected split is later than DX's midpoint: the press keeps its bottom-out.
    let press = decoded(&report, &key_files(&m, "KeyA", KeyAction::Down)[0]);
    assert!(press.len() > ms(SPRITE_RATE, SLICE_MS / 2.0 - LEAD_MS + 5.0), "{}", press.len());
    assert!(m.keys["Enter"].release.is_empty());
    assert_eq!(m.keys["Backspace"].release.len(), 1);

    // Without splitting, DX's own pairs are used as they are.
    let env = Env::new();
    let src = write_pack(&env.src("dx"), &dx_config(&defs, ""), &[("sound.wav", sprite_wav(5))]);
    let opts = ImportOptions { split_release: false, ..ImportOptions::default() };
    let report = env.import_with(&src, opts).unwrap();
    assert_eq!(report.split.eligible, 0);
    assert_eq!(report.keys_with_release, 3);
    check(&report, &[]);
}

#[test]
fn dx_backup_of_the_original_v1_config_is_preferred() {
    let env = Env::new();
    let src = write_pack(
        &env.src("converted"),
        &dx_config(&format!(r#""KeyA": {}"#, dx_pair(0)), ""),
        &[
            ("sound.wav", sprite_wav(2)),
            (
                "config.json.v1.backup",
                v1_sprite_config(
                    "Original",
                    "sound.wav",
                    &format!(r#""30": {}, "1-up": [140, 80]"#, slot(0)),
                )
                .into_bytes(),
            ),
        ],
    );
    let report = env.import(&src).unwrap();
    assert_eq!(report.format, SourceFormat::MechvibesV1Sprite);
    assert_eq!(report.name, "Original");
    assert!(report.warnings.iter().any(|w| w.contains("converted")));
    check(&report, &[]);
}

#[test]
fn unsupported_packs_are_rejected() {
    let env = Env::new();
    let cases = [
        (
            "dx-mouse",
            dx_config(
                r#""MouseLeft": {"timing": [[0, 50]]}, "MouseRight": {"timing": [[60, 90]]}"#,
                "",
            ),
            "mouse",
        ),
        (
            "pp-mouse",
            r#"{"name": "M", "defines": {"1": "l.wav", "01": "l-up.wav", "2": "r.wav"}}"#.into(),
            "mouse",
        ),
        ("v3", r#"{"name": "V3", "version": 3, "defines": {}}"#.into(), "version 3"),
        ("empty", "   ".into(), "empty"),
        ("broken", "{\"name\": ".into(), "invalid config"),
    ];
    for (name, config, expected) in cases {
        let src = write_pack(&env.src(name), &config, &[("sound.wav", click(100))]);
        let err = env.import(&src).unwrap_err();
        assert!(err.to_string().contains(expected), "{name}: {err}");
    }
    let err = env.import(&env.tmp.path().join("nothing")).unwrap_err();
    assert!(matches!(err, ImportError::Io { .. }), "{err}");
    let empty = env.src("no-config");
    fs::create_dir_all(&empty).unwrap();
    assert!(matches!(env.import(&empty).unwrap_err(), ImportError::NotAPack(_)));
    // Nothing was installed.
    assert!(fs::read_dir(env.dest()).map(|d| d.count() == 0).unwrap_or(true));
}

#[test]
fn missing_sprite_or_audio_is_reported() {
    let env = Env::new();
    let src = write_pack(
        &env.src("nosprite"),
        &v1_sprite_config("N", "gone.ogg", r#""30": [0, 10]"#),
        &[],
    );
    assert!(matches!(env.import(&src).unwrap_err(), ImportError::NoSounds(_)));
    let src = write_pack(
        &env.src("nofiles"),
        r#"{"name": "N", "key_define_type": "multi", "defines": {"30": "a.wav"}}"#,
        &[],
    );
    let err = env.import(&src).unwrap_err().to_string();
    assert!(err.contains("1 missing") && err.contains("no audio files"), "{err}");
}

#[test]
fn ids_are_unique_and_imports_are_never_overwritten_silently() {
    let env = Env::new();
    let config = r#"{"name": "Same Name", "key_define_type": "multi", "defines": {"30": "a.wav"}}"#;
    let one = write_pack(&env.src("one"), config, &[("a.wav", click(1_000))]);
    let two = write_pack(&env.src("two"), config, &[("a.wav", click(1_100))]);
    // A folder that is not a pack takes the id's name, too.
    fs::create_dir_all(env.dest().join("mv-same-name")).unwrap();
    let first = env.import(&one).unwrap();
    assert_eq!(first.id, "mv-same-name-2");
    let second = env.import(&two).unwrap();
    assert_eq!(second.id, "mv-same-name-3");

    // Importing the same pack again is refused…
    match env.import(&one).unwrap_err() {
        ImportError::AlreadyImported { id, .. } => assert_eq!(id, "mv-same-name-2"),
        other => panic!("{other}"),
    }
    // …unless asked to overwrite, which replaces the folder.
    fs::write(first.path.join("stale.txt"), "x").unwrap();
    let opts = ImportOptions { overwrite: true, ..ImportOptions::default() };
    let again = env.import_with(&one, opts).unwrap();
    assert!(again.replaced);
    assert_eq!(again.id, "mv-same-name-2");
    assert!(!again.path.join("stale.txt").exists());
    check(&again, &["release"]);
    let mut names: Vec<String> = fs::read_dir(env.dest())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, ["mv-same-name", "mv-same-name-2", "mv-same-name-3"]);
}

#[test]
fn zip_inputs() {
    let env = Env::new();
    fs::create_dir_all(env.src("")).unwrap();
    let config = r#"{"name": "Zipped", "key_define_type": "multi",
        "defines": {"30": "a.wav", "57": "sounds/space.wav"}}"#;
    // Archive Utility layout: one top folder, __MACOSX junk, a stray readme.
    let nested = write_zip(
        &env.src("nested.zip"),
        &[
            ("Zipped Pack/config.json", config.as_bytes().to_vec()),
            ("Zipped Pack/a.wav", click(1_000)),
            ("Zipped Pack/sounds/space.wav", click(1_100)),
            ("__MACOSX/Zipped Pack/._config.json", b"\0\x05\x16\x07junk".to_vec()),
            ("Zipped Pack/.DS_Store", b"junk".to_vec()),
            ("readme.txt", b"hello".to_vec()),
        ],
    );
    let report = env.import(&nested).unwrap();
    assert_eq!(report.source, "nested.zip");
    assert_eq!(report.keys_mapped.len(), 2);
    check(&report, &["release"]);

    // Flat, with a BOM and an upper-case CONFIG.JSON.
    let mut bom = b"\xEF\xBB\xBF".to_vec();
    bom.extend_from_slice(config.replace("Zipped", "Flat").as_bytes());
    let flat = write_zip(
        &env.src("flat.zip"),
        &[("CONFIG.JSON", bom), ("a.wav", click(1_000)), ("sounds/space.wav", click(1_100))],
    );
    let report = env.import(&flat).unwrap();
    assert_eq!(report.id, "mv-flat");
    check(&report, &["release"]);

    let two = write_zip(
        &env.src("two.zip"),
        &[
            ("a/config.json", config.as_bytes().to_vec()),
            ("b/config.json", config.as_bytes().to_vec()),
        ],
    );
    assert!(env.import(&two).unwrap_err().to_string().contains("more than one pack"));

    let config_only = write_zip(&env.src("v2.zip"), &[("config.json", config.as_bytes().to_vec())]);
    let err = env.import(&config_only).unwrap_err().to_string();
    assert!(err.contains("only config.json"), "{err}");
}

#[test]
fn long_sounds_are_truncated_and_long_names_shortened() {
    let env = Env::new();
    let name =
        "Every keypress is a very long sound - with a name longer than sixty-four characters";
    let config = format!(
        r#"{{"name": "{name}", "key_define_type": "multi", "defines": {{"30": "long.wav"}}}}"#
    );
    let src = write_pack(&env.src("long"), &config, &[("long.wav", wav(48_000, 0, 48_000 * 3))]);
    let report = env.import(&src).unwrap();
    assert!(report.warnings.iter().any(|w| w.contains("cut short")));
    assert!(report.warnings.iter().any(|w| w.contains("shortened")));
    let m = check(&report, &["release"]);
    assert_eq!(m.name.chars().count(), 64);
    assert!(m.description.clone().unwrap().contains(name));
    assert!(m.id.len() <= 64 && m.id.starts_with("mv-every-keypress"));
    let press = decoded(&report, &key_files(&m, "KeyA", KeyAction::Down)[0]);
    assert!(press.len() < 2 * 48_000);
}
