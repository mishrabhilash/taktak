//! CI gate for the packs TakTak ships (`<repo>/packs`): the "Bundled packs" rules of
//! docs/pack-format.md. Every pack is checked and every failure reported in one run.

#[path = "common/sha256.rs"]
mod sha256;

use sha256::sha256_hex;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;
use taktak_core::input::KeyAction;
use taktak_core::key::Key;
use taktak_core::pack::{self, PackOrigin};

const RATE: u32 = 48_000;
const MAX_PACK_BYTES: u64 = 6 * 1024 * 1024;

/// Third-party trademarks a bundled pack must not use in its name or id (the list in
/// docs/pack-format.md). Matched case-insensitively on whole words.
const TRADEMARKS: [&str; 14] = [
    "IBM", "Model M", "Model F", "Cherry", "MX", "Gateron", "Kailh", "Keychron", "Razer",
    "Logitech", "Corsair", "Topre", "HHKB", "Kenney",
];

/// Licenses whose terms require their notice (or the credit and a license link) to travel with
/// every copy, so the pack must ship LICENSE.txt.
const NOTICE_LICENSES: [&str; 7] =
    ["MIT", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Apache-2.0", "CC-BY-3.0", "CC-BY-4.0"];

fn repo_file(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel)
}

/// Audio file extensions, as the loader accepts them (matched ignoring case), plus formats an
/// author might drop in by mistake: any audio in a bundled pack needs its provenance.
const AUDIO_EXTENSIONS: [&str; 5] = ["wav", "ogg", "mp3", "flac", "aiff"];

/// Direct child folders of `<repo>/packs`, sorted. Hidden entries are skipped, as the app
/// skips them.
fn bundled_pack_folders() -> Vec<PathBuf> {
    let dir = repo_file("packs");
    let entries = fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    let mut folders: Vec<PathBuf> = entries
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        .filter(|path| !path.file_name().unwrap().to_string_lossy().starts_with('.'))
        .collect();
    folders.sort();
    folders
}

/// Entries of `dir` the app would load as packs but this gate does not check: `.zip` files
/// (any letter case), which the registry treats as packs just like folders.
fn unchecked_candidates(dir: &Path) -> Vec<String> {
    let entries = fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    let mut names: Vec<String> = entries
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| !name.starts_with('.'))
        .filter(|name| {
            Path::new(name).extension().is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
        })
        .collect();
    names.sort();
    names
}

/// Pack-relative paths (with `/`) of every audio file under `dir`, sorted. Hidden entries are
/// included: whatever ships needs provenance.
fn audio_files(dir: &Path) -> Vec<String> {
    fn walk(root: &Path, rel: &str, out: &mut Vec<String>) {
        for entry in fs::read_dir(root.join(rel)).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            let child = if rel.is_empty() { name } else { format!("{rel}/{name}") };
            if fs::metadata(entry.path()).unwrap().is_dir() {
                walk(root, &child, out);
            } else if Path::new(&child).extension().is_some_and(|ext| {
                AUDIO_EXTENSIONS.iter().any(|known| ext.eq_ignore_ascii_case(known))
            }) {
                out.push(child);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, "", &mut out);
    out.sort();
    out
}

/// Whether `line` names `path` as a whole path: not as the tail of a longer one
/// (`sounds/a.wav` in `sounds/aa.wav` or `x/sounds/a.wav`).
fn names_path(line: &str, path: &str) -> bool {
    let is_path_char = |c: char| c.is_alphanumeric() || "/._-".contains(c);
    line.match_indices(path).any(|(at, _)| {
        let before = line[..at].chars().next_back();
        let after = line[at + path.len()..].chars().next();
        !before.is_some_and(is_path_char) && !after.is_some_and(is_path_char)
    })
}

/// The SOURCES.md rule: every audio file in the pack is listed by its pack-relative path with
/// the SHA-256 of the file as shipped, on one line (a table row). Every miss is reported.
fn provenance_problems(folder: &Path, sources: &str) -> Vec<String> {
    let lines: Vec<&str> = sources.lines().collect();
    audio_files(folder)
        .into_iter()
        .filter_map(|rel| {
            let digest = sha256_hex(&fs::read(folder.join(&rel)).unwrap());
            let recorded = lines
                .iter()
                .any(|line| names_path(line, &rel) && line.to_ascii_lowercase().contains(&digest));
            (!recorded).then(|| {
                format!(
                    "SOURCES.md does not record {rel} with its SHA-256 as shipped ({digest}) on \
                     one line"
                )
            })
        })
        .collect()
}

/// Lowercase words: `"Model-M Clicky"` → `["model", "m", "clicky"]`.
fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// The trademarks `text` uses as whole words (a multi-word mark as consecutive words).
fn trademarks_in(text: &str) -> Vec<&'static str> {
    let text = words(text);
    TRADEMARKS
        .into_iter()
        .filter(|mark| {
            let mark = words(mark);
            text.windows(mark.len()).any(|window| window == mark.as_slice())
        })
        .collect()
}

fn size_on_disk(dir: &Path) -> u64 {
    fs::read_dir(dir)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            let meta = fs::metadata(entry.path()).unwrap();
            if meta.is_dir() { size_on_disk(&entry.path()) } else { meta.len() }
        })
        .sum()
}

/// Every rule a bundled pack breaks (empty when it passes).
fn problems_with(folder: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    let folder_name = folder.file_name().unwrap().to_string_lossy().into_owned();

    match pack::inspect(folder, PackOrigin::Bundled, true) {
        Ok((info, warnings)) => {
            for warning in &warnings {
                println!("{folder_name}: {warning}");
            }
            if info.id != folder_name {
                problems.push(format!("folder name must equal the pack id {:?}", info.id));
            }
            for (field, text) in [("name", &info.name), ("id", &info.id)] {
                let marks = trademarks_in(text);
                if !marks.is_empty() {
                    problems.push(format!(
                        "{field} {text:?} uses third-party trademarks {marks:?}; name the hardware \
                         in the description instead"
                    ));
                }
            }
            if NOTICE_LICENSES.contains(&info.license.as_str())
                && !folder.join("LICENSE.txt").is_file()
            {
                problems.push(format!("{} requires LICENSE.txt in the pack folder", info.license));
            }
        }
        Err(e) => problems.push(format!("strict validation failed:\n{e}")),
    }

    let started = Instant::now();
    match pack::load(folder, PackOrigin::Bundled, RATE) {
        Ok(loaded) => {
            println!(
                "{folder_name}: loaded {} sounds in {:.0} ms",
                loaded.bank.samples.len(),
                started.elapsed().as_secs_f64() * 1000.0
            );
            let silent: Vec<&str> = Key::ALL
                .iter()
                .filter(|&&key| loaded.bank.map.get(key, KeyAction::Down).is_empty())
                .map(|key| key.code_name())
                .collect();
            if !silent.is_empty() {
                problems.push(format!("keys without a press sound: {silent:?}"));
            }
        }
        Err(e) => problems.push(format!("loading at {RATE} Hz failed:\n{e}")),
    }

    match fs::read_to_string(folder.join("SOURCES.md")) {
        Ok(sources) if !sources.trim().is_empty() => {
            problems.extend(provenance_problems(folder, &sources));
        }
        _ => problems.push(
            "SOURCES.md is missing or empty: list every file's origin, author, license proof, \
             SHA-256 and processing"
                .to_owned(),
        ),
    }

    let size = size_on_disk(folder);
    if size > MAX_PACK_BYTES {
        problems.push(format!(
            "pack is {:.2} MB on disk; bundled packs are limited to {} MB",
            size as f64 / (1024.0 * 1024.0),
            MAX_PACK_BYTES / (1024 * 1024)
        ));
    }
    problems
}

#[test]
fn bundled_packs_follow_the_bundle_rules() {
    let unchecked = unchecked_candidates(&repo_file("packs"));
    assert!(
        unchecked.is_empty(),
        "packs/ must hold only pack folders: the app would load {unchecked:?} as packs, but this \
         gate does not check them; ship the pack as a folder"
    );
    let folders = bundled_pack_folders();
    assert!(
        !folders.is_empty(),
        "no bundled packs in {}: TakTak must ship at least one pack",
        repo_file("packs").display()
    );
    let failures: Vec<String> = folders
        .iter()
        .filter_map(|folder| {
            // Multi-line problems (a PackError listing) are indented under their bullet.
            let problems: Vec<String> =
                problems_with(folder).iter().map(|p| p.replace('\n', "\n    ")).collect();
            (!problems.is_empty()).then(|| {
                format!(
                    "{}:\n  - {}",
                    folder.file_name().unwrap().to_string_lossy(),
                    problems.join("\n  - ")
                )
            })
        })
        .collect();
    assert!(
        failures.is_empty(),
        "{} of {} bundled packs break the rules in docs/pack-format.md (Bundled packs):\n\n{}",
        failures.len(),
        folders.len(),
        failures.join("\n\n")
    );
}

#[test]
fn provenance_needs_each_file_with_its_shipped_hash() {
    let tmp = tempfile::tempdir().unwrap();
    let pack = tmp.path();
    fs::create_dir_all(pack.join("sounds")).unwrap();
    fs::write(pack.join("sounds/a.wav"), b"aaa").unwrap();
    fs::write(pack.join("sounds/B.WAV"), b"bbb").unwrap();
    fs::write(pack.join("preview.wav"), b"ppp").unwrap();
    fs::write(pack.join("notes.txt"), b"not audio").unwrap();
    let (a, b, p) = (sha256_hex(b"aaa"), sha256_hex(b"bbb"), sha256_hex(b"ppp"));
    let complete = format!(
        "| file | sha256 |\n|---|---|\n| `sounds/a.wav` | `{a}` |\n| `sounds/B.WAV` | {} |\n\
         preview.wav: {p}\n",
        b.to_uppercase()
    );
    assert_eq!(provenance_problems(pack, &complete), Vec::<String>::new());

    // A new file nobody recorded, and a file rebuilt without updating its hash.
    fs::write(pack.join("sounds/new.wav"), b"new").unwrap();
    fs::write(pack.join("sounds/a.wav"), b"quieter").unwrap();
    let found = provenance_problems(pack, &complete);
    assert_eq!(found.len(), 2, "{found:#?}");
    assert!(found[0].contains("sounds/a.wav") && found[0].contains(&sha256_hex(b"quieter")));
    assert!(found[1].contains("sounds/new.wav"));

    // The path and the hash must be on the same line, and the path must be whole.
    fs::write(pack.join("sounds/a.wav"), b"aaa").unwrap();
    fs::remove_file(pack.join("sounds/new.wav")).unwrap();
    let split =
        complete.replace(&format!("| `sounds/a.wav` | `{a}` |"), &format!("sounds/a.wav\n{a}"));
    assert_eq!(provenance_problems(pack, &split).len(), 1);
    let longer = complete.replace("`sounds/a.wav`", "`sounds/a.wav.bak`");
    assert_eq!(provenance_problems(pack, &longer).len(), 1);
}

#[test]
fn zips_in_the_packs_folder_are_flagged() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("folder-pack")).unwrap();
    fs::write(tmp.path().join("README.md"), b"").unwrap();
    fs::write(tmp.path().join(".hidden.zip"), b"").unwrap();
    assert!(unchecked_candidates(tmp.path()).is_empty());
    fs::write(tmp.path().join("cherry-mx.ZIP"), b"").unwrap();
    assert_eq!(unchecked_candidates(tmp.path()), ["cherry-mx.ZIP"]);
}

#[test]
fn trademarks_match_whole_words_only() {
    assert_eq!(trademarks_in("Model M Buckling"), ["Model M"]);
    assert_eq!(trademarks_in("model-m"), ["Model M"]);
    assert_eq!(trademarks_in("Cherry MX Blue"), ["Cherry", "MX"]);
    assert_eq!(trademarks_in("ibm-1391401"), ["IBM"]);
    assert_eq!(trademarks_in("hhkb-topre"), ["Topre", "HHKB"]);
    for clean in ["Deep Thock", "Mixed Clack", "Modem Click", "Model Mm", "Cherrywood", "Kenneys"] {
        assert!(trademarks_in(clean).is_empty(), "{clean}");
    }
}

#[test]
fn trademark_list_matches_the_spec() {
    let spec = fs::read_to_string(repo_file("docs/pack-format.md")).unwrap();
    let section = spec.split("## Bundled packs").nth(1).expect("spec has a Bundled packs section");
    for mark in TRADEMARKS {
        assert!(section.contains(mark), "{mark} is not listed in docs/pack-format.md");
    }
}
