//! End-to-end pack loading: folder and zip packs on disk → `inspect` / `load` → `SoundBank`,
//! and the registry running the real inspector.

mod common;

use common::{RATE, click, fixture, folder, manifest, manifest_with, wav, write_folder, write_zip};
use std::path::Path;
use taktak_core::input::KeyAction::{self, Down, Up};
use taktak_core::key::Key;
use taktak_core::pack::decode::leading_silence;
use taktak_core::pack::{
    self, LoadedPack, PackError, PackOrigin, PackRegistry, Problem, RegistryEvent, Severity,
};

fn load(path: &Path) -> LoadedPack {
    pack::load(path, PackOrigin::User, RATE).unwrap_or_else(|e| panic!("{e}"))
}

fn load_err(path: &Path) -> PackError {
    match pack::load(path, PackOrigin::User, RATE) {
        Ok(_) => panic!("{} loaded", path.display()),
        Err(e) => e,
    }
}

fn inspect_err(path: &Path, strict: bool) -> PackError {
    match pack::inspect(path, PackOrigin::User, strict) {
        Ok(_) => panic!("{} passed inspection", path.display()),
        Err(e) => e,
    }
}

fn located(problems: &[Problem]) -> Vec<(Severity, &str)> {
    problems.iter().map(|p| (p.severity, p.location.as_str())).collect()
}

// ---- resolution end to end ------------------------------------------------------------------

const RESOLUTION_PACK: &str = r#""volume": 0.5,
  "variation": { "pitch": 0.05, "wobble": 1 },
  "groups": {
    "alphanumeric": { "press": ["sounds/a1.wav", "sounds/a2.wav", "sounds/a3.wav"],
                      "release": ["sounds/up.wav"] },
    "space":        { "press": ["sounds/space.mp3"] },
    "other":        { "press": ["sounds/other.ogg"] },
    "modifiers":    { "release": ["sounds/mod-up.wav"] }
  },
  "keys": {
    "KeyA":  { "press": ["sounds/keya.wav"] },
    "Enter": { "press": ["sounds/keya.wav", "sounds/keya.wav"], "release": [] }
  }"#;

/// The distinct sound files of `RESOLUTION_PACK` in path order, which is `bank.samples` order.
const ORDER: [&str; 8] = [
    "sounds/a1.wav",
    "sounds/a2.wav",
    "sounds/a3.wav",
    "sounds/keya.wav",
    "sounds/mod-up.wav",
    "sounds/other.ogg",
    "sounds/space.mp3",
    "sounds/up.wav",
];

/// WAV clicks of distinct lengths (so a decoded sample identifies its file), plus the MP3 and
/// Ogg Vorbis fixtures (0.1 s at 44.1 kHz).
fn resolution_files() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("sounds/a1.wav", click(1000)),
        ("sounds/a2.wav", click(1100)),
        ("sounds/a3.wav", click(1200)),
        ("sounds/up.wav", click(1300)),
        ("sounds/keya.wav", click(1400)),
        ("sounds/mod-up.wav", click(1500)),
        ("sounds/space.mp3", fixture("tone-click-mono-44100.mp3")),
        ("sounds/other.ogg", fixture("tone-click-stereo-44100.ogg")),
    ]
}

fn sounds(loaded: &LoadedPack, key: Key, action: KeyAction) -> Vec<&'static str> {
    loaded.bank.map.get(key, action).iter().map(|&i| ORDER[i as usize]).collect()
}

fn assert_resolution(loaded: &LoadedPack) {
    let bank = &loaded.bank;
    assert_eq!(bank.samples.len(), ORDER.len(), "each distinct file is decoded once");
    let lengths: Vec<usize> = bank.samples.iter().map(|s| s.len()).collect();
    assert_eq!(lengths[..5], [1000, 1100, 1200, 1400, 1500]);
    assert_eq!(lengths[7], 1300);
    for (i, name) in [(5, "ogg"), (6, "mp3")] {
        // About 4410 samples at 44.1 kHz is about 4800 at 48 kHz, minus what trimming cut.
        assert!((4700..=4820).contains(&lengths[i]), "{name}: {} samples", lengths[i]);
    }

    let alnum = ["sounds/a1.wav", "sounds/a2.wav", "sounds/a3.wav"];
    let up = ["sounds/up.wav"];
    let other = ["sounds/other.ogg"];
    let cases: [(Key, &[&str], &[&str]); 11] = [
        // Per-key press; release falls back independently to the group.
        (Key::KeyA, &["sounds/keya.wav"], &up),
        // Group with variants.
        (Key::KeyB, &alnum, &up),
        (Key::Digit5, &alnum, &up),
        (Key::Slash, &alnum, &up),
        // space.press, then release walks space → other → alphanumeric.
        (Key::Space, &["sounds/space.mp3"], &up),
        (Key::F1, &other, &up),
        (Key::ArrowUp, &other, &up),
        // modifiers has release only: press falls back to other.
        (Key::ShiftLeft, &other, &["sounds/mod-up.wav"]),
        (Key::CapsLock, &other, &["sounds/mod-up.wav"]),
        // Duplicates are kept (a weighted choice); an empty array does not stop the chain.
        (Key::Enter, &["sounds/keya.wav", "sounds/keya.wav"], &up),
        (Key::NumpadEnter, &other, &up),
    ];
    for (key, press, release) in cases {
        assert_eq!(sounds(loaded, key, Down), press, "{} press", key.code_name());
        assert_eq!(sounds(loaded, key, Up), release, "{} release", key.code_name());
    }
    for &key in Key::ALL {
        assert!(!bank.map.get(key, Down).is_empty(), "{} is silent", key.code_name());
    }

    assert_eq!(bank.gain, 0.5);
    assert_eq!((bank.variation.pitch, bank.variation.volume), (0.05, 0.10));
    // No preview field: the first alphanumeric press, at the pack's volume (the bank's copy
    // stays as decoded; the mixer applies the volume to key sounds).
    let preview = loaded.preview.as_deref().expect("a preview");
    let first = &*bank.samples[0];
    assert_eq!(preview.len(), first.len());
    assert!(preview.iter().zip(first).all(|(p, s)| *p == s * 0.5));
    // Unknown fields inside "variation" are ignored without a warning.
    assert_eq!(loaded.warnings, []);
}

#[test]
fn folder_pack_resolves_every_key_end_to_end() {
    let (_tmp, path) = folder(&manifest(RESOLUTION_PACK), &resolution_files());
    let loaded = load(&path);
    assert_resolution(&loaded);
    assert_eq!(loaded.info.id, "test");
    assert_eq!(loaded.info.name, "Test pack");
    assert_eq!(loaded.info.location, path);
    assert_eq!(loaded.info.origin, PackOrigin::User);

    let (info, warnings) = pack::inspect(&path, PackOrigin::Bundled, true).unwrap();
    assert_eq!(info, pack::PackInfo { origin: PackOrigin::Bundled, ..loaded.info.clone() });
    assert_eq!(warnings, []);
}

#[test]
fn zip_pack_in_a_top_level_folder_loads_like_the_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let zip = write_zip(
        &tmp.path().join("my-pack.zip"),
        "my-pack/",
        &manifest(RESOLUTION_PACK),
        &resolution_files(),
        &[
            ("README.md", b"Stray files next to the pack folder are ignored."),
            ("__MACOSX/my-pack/._pack.json", b"resource fork"),
        ],
    );
    let loaded = load(&zip);
    assert_resolution(&loaded);
    assert_eq!(loaded.info.location, zip);

    let flat = write_zip(
        &tmp.path().join("flat.ZIP"),
        "",
        &manifest(RESOLUTION_PACK),
        &resolution_files(),
        &[],
    );
    assert_resolution(&load(&flat));
}

#[test]
fn keys_without_any_release_sound_are_silent_on_release() {
    let (_tmp, path) = folder(
        &manifest(
            r#""groups": { "other": { "press": ["p.wav"] }, "space": { "release": ["r.wav"] } }"#,
        ),
        &[("p.wav", click(500)), ("r.wav", click(600))],
    );
    let loaded = load(&path);
    let map = &loaded.bank.map;
    assert_eq!(map.get(Key::KeyA, Down), [0]);
    assert_eq!(map.get(Key::Space, Down), [0]);
    assert_eq!(map.get(Key::Space, Up), [1]);
    for key in [Key::KeyA, Key::Enter, Key::ShiftLeft, Key::F12] {
        assert!(map.get(key, Up).is_empty(), "{}", key.code_name());
    }
    assert_eq!(loaded.bank.gain, 1.0);
    assert_eq!((loaded.bank.variation.pitch, loaded.bank.variation.volume), (0.03, 0.10));
    // Fallback preview: no alphanumeric group, so the first "other" press.
    assert_eq!(loaded.preview.as_deref().map(<[f32]>::len), Some(500));
}

#[test]
fn explicit_preview_is_decoded_but_kept_out_of_the_bank() {
    let files = [("p.wav", click(500)), ("demo.wav", click(9000))];
    let (_tmp, path) = folder(
        &manifest(r#""preview": "demo.wav", "groups": { "other": { "press": ["p.wav"] } }"#),
        &files,
    );
    let loaded = load(&path);
    assert_eq!(loaded.bank.samples.len(), 1);
    assert_eq!(loaded.preview.as_deref().map(<[f32]>::len), Some(9000));

    // A preview that is also a key sound reuses the decoded sample.
    let (_tmp, path) = folder(
        &manifest(r#""preview": "p.wav", "groups": { "other": { "press": ["p.wav"] } }"#),
        &files,
    );
    let loaded = load(&path);
    assert_eq!(loaded.preview.as_deref(), Some(&*loaded.bank.samples[0]));
}

// ---- trimming ----------------------------------------------------------------------------------

fn single_sound_pack(fields: &str, bytes: Vec<u8>) -> (tempfile::TempDir, std::path::PathBuf) {
    let set = r#"{ "press": ["sounds/late.wav"], "release": ["sounds/late.wav"] }"#;
    let fields = format!(r#"{fields}"groups": {{ "other": {set} }}"#);
    folder(&manifest(&fields), &[("sounds/late.wav", bytes)])
}

#[test]
fn leading_silence_is_trimmed_to_the_pre_roll() {
    // 10 ms of silence at 48 kHz: cut to the 0.5 ms (24-sample) pre-roll.
    let (_tmp, path) = single_sound_pack("", wav(RATE, 480, 2400));
    let loaded = load(&path);
    let sample = &loaded.bank.samples[0];
    assert_eq!(leading_silence(sample), 24);
    assert_eq!(sample.len(), 24 + 2400);
    assert_eq!(loaded.warnings, []);

    // At 44.1 kHz the cut happens before resampling, and the onset still lands on the
    // pre-roll at the output rate.
    let (_tmp, path) = single_sound_pack("", wav(44_100, 441, 2205));
    let sample = &load(&path).bank.samples[0];
    let (onset, _) =
        sample.iter().enumerate().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).unwrap();
    assert!((23..=25).contains(&onset), "onset at {onset}");
    // Resampling rings a little ahead of a sharp onset, but never before the trimmed start.
    assert!(leading_silence(sample) <= 24);
    assert_eq!(sample.len(), 2424, "round((22 + 2205) * 48000 / 44100)");
}

#[test]
fn untrimmed_leading_silence_is_kept_and_warned_about() {
    let (_tmp, path) = single_sound_pack(r#""trim_silence": false, "#, wav(RATE, 480, 2400));
    let loaded = load(&path);
    assert_eq!(leading_silence(&loaded.bank.samples[0]), 480);
    assert_eq!(located(&loaded.warnings), [(Severity::Warning, "sounds/late.wav")]);
    assert!(
        loaded.warnings[0].message.starts_with("starts with 10.0 ms of silence"),
        "{}",
        loaded.warnings[0]
    );

    // 4 ms is under the 5 ms threshold.
    let (_tmp, path) = single_sound_pack(r#""trim_silence": false, "#, wav(RATE, 192, 2400));
    let loaded = load(&path);
    assert_eq!(leading_silence(&loaded.bank.samples[0]), 192);
    assert_eq!(loaded.warnings, []);
}

// ---- errors --------------------------------------------------------------------------------------

const BROKEN_REFERENCES: &str = r#""groups": {
    "alphanumeric": { "press": ["sounds/ok.wav"] },
    "space": { "press": ["sounds/ok.wav", "sounds/gone.wav"], "release": ["sounds/click.wav"] }
  },
  "keys": {
    "KeyA": { "release": ["sounds/gone.wav"] },
    "KeyB": { "press": ["sounds/ok.wav:stream"] },
    "keya": { "press": ["sounds/ok.wav"] }
  }"#;

fn assert_broken_references(err: &PackError) {
    let got: Vec<(&str, &str)> =
        err.problems.iter().map(|p| (p.location.as_str(), p.message.as_str())).collect();
    assert!(err.problems.iter().all(Problem::is_error), "{err}");
    assert_eq!(got.len(), 5, "{err}");
    // Syntax and naming errors from validation first, then every reference to a missing file.
    assert_eq!(got[0].0, "keys.KeyB.press[0]");
    assert!(got[0].1.contains("':'"), "{}", got[0].1);
    assert_eq!(got[1].0, "keys.keya");
    assert_eq!(
        got[2..],
        [
            ("groups.space.press[1]", "\"sounds/gone.wav\": file not found in the pack"),
            (
                "groups.space.release[0]",
                "\"sounds/click.wav\": file not found; names are case-sensitive and the pack has \
                 \"sounds/Click.wav\""
            ),
            ("keys.KeyA.release[0]", "\"sounds/gone.wav\": file not found in the pack"),
        ]
    );
}

#[test]
fn missing_and_miscased_files_are_reported_at_every_reference() {
    let files = [("sounds/ok.wav", click(500)), ("sounds/Click.wav", click(500))];
    let (tmp, path) = folder(&manifest(BROKEN_REFERENCES), &files);
    let err = inspect_err(&path, false);
    assert_eq!(err.pack, path);
    assert_broken_references(&err);
    assert_broken_references(&load_err(&path));

    let zip = write_zip(
        &tmp.path().join("broken.zip"),
        "broken/",
        &manifest(BROKEN_REFERENCES),
        &files,
        &[],
    );
    assert_broken_references(&inspect_err(&zip, false));
}

#[test]
fn decode_problems_are_all_reported_by_load() {
    let (_tmp, path) = folder(
        &manifest(
            r#""groups": {
                "alphanumeric": { "press": ["sounds/ok.wav", "sounds/long.wav", "sounds/bad.wav"] },
                "other": { "press": ["sounds/bad.ogg", "sounds/two.wav"] } }"#,
        ),
        &[
            ("sounds/ok.wav", click(500)),
            ("sounds/long.wav", click(120_000)),
            ("sounds/two.wav", click(96_000)),
            ("sounds/bad.wav", b"RIFF\x10\0\0\0WAVEjunk".to_vec()),
            ("sounds/bad.ogg", b"not an ogg file at all".repeat(10)),
        ],
    );
    // Inspection does not decode, so it passes.
    let (_, warnings) = pack::inspect(&path, PackOrigin::User, true).unwrap();
    assert_eq!(located(&warnings), [(Severity::Warning, "release")]);

    let err = load_err(&path);
    assert_eq!(
        located(&err.problems),
        [
            (Severity::Error, "sounds/bad.ogg"),
            (Severity::Error, "sounds/bad.wav"),
            (Severity::Error, "sounds/long.wav"),
            (Severity::Warning, "release"),
        ],
        "{err}"
    );
    assert!(err.problems[0].message.starts_with("not a valid Ogg Vorbis file: "), "{err}");
    assert!(err.problems[1].message.starts_with("not a valid WAV file: "), "{err}");
    assert_eq!(err.problems[2].message, "sound is 2.50 s long; sounds must be at most 2 s");
}

#[test]
fn too_many_distinct_files_is_one_error() {
    let list = |n: usize| (0..n).map(|i| format!("\"s/{i}.wav\"")).collect::<Vec<_>>().join(",");
    let fields = format!(r#""groups": {{ "other": {{ "press": [{}, "s/0.wav"] }} }}"#, list(2001));
    let (_tmp, path) = folder(&manifest(&fields), &[]);
    let err = inspect_err(&path, false);
    assert_eq!(
        err.problems.iter().filter(|p| p.is_error()).cloned().collect::<Vec<_>>(),
        [Problem::error("pack", "pack references 2001 distinct audio files; the limit is 2000")]
    );

    // At the limit, the files themselves are checked (and here, all missing).
    let fields = format!(r#""groups": {{ "other": {{ "press": [{}] }} }}"#, list(2000));
    let (_tmp, path) = folder(&manifest(&fields), &[]);
    let err = inspect_err(&path, false);
    assert_eq!(err.errors().count(), 2000);
    assert!(err.errors().all(|p| p.message.ends_with("file not found in the pack")));
}

#[test]
fn unreadable_manifests_and_sources_are_pack_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let err = inspect_err(&tmp.path().join("nope"), false);
    assert_eq!(located(&err.problems), [(Severity::Error, "pack")]);

    let path = write_folder(&tmp.path().join("bad-json"), "{ \"format\": 1,", &[]);
    let err = load_err(&path);
    assert_eq!(err.problems.len(), 1);
    assert!(err.problems[0].location.starts_with("pack.json:"), "{err}");
}

#[test]
fn personal_license_loads_but_fails_strict_inspection() {
    let (_tmp, path) = folder(
        &manifest_with(
            "mine",
            "LicenseRef-Personal",
            r#""groups": { "other": { "press": ["a.wav"], "release": ["a.wav"] } }"#,
        ),
        &[("a.wav", click(500))],
    );
    let (_, warnings) = pack::inspect(&path, PackOrigin::User, false).unwrap();
    assert_eq!(located(&warnings), [(Severity::Warning, "license")]);
    let err = inspect_err(&path, true);
    assert_eq!(located(&err.problems), [(Severity::Error, "license")]);
    assert!(err.problems[0].message.contains("personal use only"), "{err}");

    let loaded = load(&path);
    assert_eq!(located(&loaded.warnings), [(Severity::Warning, "license")]);
}

// ---- registry ------------------------------------------------------------------------------------

#[test]
fn registry_scans_real_packs_end_to_end() {
    let tmp = tempfile::tempdir().unwrap();
    let (bundled, user) = (tmp.path().join("bundled"), tmp.path().join("user"));
    let good =
        |id: &str| manifest_with(id, "CC0-1.0", r#""groups": { "other": { "press": ["a.wav"] } }"#);
    let files = [("a.wav", click(500))];
    write_folder(&bundled.join("alpha"), &good("alpha"), &files);
    write_folder(&bundled.join("gamma"), &good("gamma"), &files);
    std::fs::create_dir_all(&user).unwrap();
    write_zip(&user.join("beta.zip"), "beta/", &good("beta"), &files, &[]);
    write_folder(&user.join("broken"), &good("broken"), &[]);
    let my_gamma = write_folder(&user.join("my-gamma"), &good("gamma"), &files);

    let mut registry = PackRegistry::new(Some(bundled.clone()), Some(user.clone()));
    let events = registry.scan();
    let summary: Vec<String> = events
        .iter()
        .map(|e| match e {
            RegistryEvent::Added(info) => format!("added {}", info.id),
            RegistryEvent::Updated(info) => format!("updated {}", info.id),
            RegistryEvent::Removed { id, .. } => format!("removed {id}"),
            RegistryEvent::Invalid(err) => {
                format!("invalid {}", err.pack.file_name().unwrap().to_string_lossy())
            }
            RegistryEvent::InvalidCleared { location } => {
                format!("cleared {}", location.file_name().unwrap().to_string_lossy())
            }
        })
        .collect();
    assert_eq!(summary, ["added alpha", "added beta", "added gamma", "invalid broken"]);
    let RegistryEvent::Invalid(err) = &events[3] else { unreachable!() };
    assert_eq!(
        located(&err.problems),
        [(Severity::Error, "groups.other.press[0]"), (Severity::Warning, "release")]
    );

    let packs = registry.packs();
    let ids: Vec<&str> = packs.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["alpha", "beta", "gamma"]);
    let gamma = registry.get("gamma").unwrap();
    assert_eq!((gamma.origin, gamma.location.as_path()), (PackOrigin::User, my_gamma.as_path()));
    for info in &packs {
        let loaded = pack::load(&info.location, info.origin, RATE).unwrap();
        assert_eq!(loaded.info.id, info.id);
    }

    // Fixing the broken pack is picked up by the next scan, and its error is withdrawn.
    std::fs::write(user.join("broken/a.wav"), click(500)).unwrap();
    let events = registry.scan();
    assert!(
        matches!(
            &events[..],
            [RegistryEvent::InvalidCleared { location }, RegistryEvent::Added(info)]
                if info.id == "broken" && location == &user.join("broken")
        ),
        "{events:?}"
    );
}
