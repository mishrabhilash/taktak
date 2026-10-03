//! Pack metadata checks, the manifest, and writing a built pack to disk.

use crate::assemble::BuiltPack;
use crate::wav;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use taktak_core::pack::manifest::{ALLOWED_LICENSES, FORMAT_VERSION, Manifest, PERSONAL_LICENSE};

pub const PREVIEW_FILE: &str = "preview.wav";

#[derive(Clone, Debug)]
pub struct PackMeta {
    pub id: String,
    pub name: String,
    pub author: String,
    pub license: String,
    pub version: String,
    pub description: Option<String>,
    pub attribution: Option<String>,
    pub source: String,
}

impl PackMeta {
    /// Checks the fields pack.json requires (docs/pack-format.md) so that problems surface
    /// before anything is recorded. Trims the text fields, drops blank optional ones and
    /// fills in a credit line for CC-BY licenses if none was given. Returns notes for the
    /// user.
    pub fn check(&mut self) -> Result<Vec<String>, String> {
        let mut notes = Vec::new();
        if !valid_id(&self.id) {
            return Err(format!(
                "--id {:?}: use 1-64 lowercase letters or digits in hyphen-separated words, \
                 e.g. \"my-keyboard\"",
                self.id
            ));
        }
        self.name = self.name.trim().to_owned();
        self.author = self.author.trim().to_owned();
        self.version = self.version.trim().to_owned();
        self.source = self.source.trim().to_owned();
        self.description = non_blank(self.description.take());
        // A blank credit line is no credit line: TakTak refuses a CC-BY pack without one.
        self.attribution = non_blank(self.attribution.take());
        check_len("--name", &self.name, 1, 64)?;
        check_len("--author", &self.author, 1, 128)?;
        check_len("--version", &self.version, 0, 32)?;
        if self.license == PERSONAL_LICENSE {
            notes.push(format!(
                "{PERSONAL_LICENSE}: the pack loads on your machine (with a warning) but cannot \
                 be bundled or shared in the gallery"
            ));
        } else if !ALLOWED_LICENSES.contains(&self.license.as_str()) {
            return Err(format!(
                "--license {:?}: use one of {}, or {PERSONAL_LICENSE} for personal use",
                self.license,
                ALLOWED_LICENSES.join(", ")
            ));
        }
        if self.license.starts_with("CC-BY") && self.attribution.is_none() {
            let credit = format!("{} by {}", self.name, self.author);
            notes.push(format!(
                "{} requires a credit line; using {credit:?} (see --attribution)",
                self.license
            ));
            self.attribution = Some(credit);
        }
        for (flag, v) in
            [("--description", &self.description), ("--attribution", &self.attribution)]
        {
            if let Some(v) = v {
                check_len(flag, v, 0, 500)?;
            }
        }
        check_len("--source", &self.source, 0, 500)?;
        Ok(notes)
    }
}

fn non_blank(text: Option<String>) -> Option<String> {
    text.map(|t| t.trim().to_owned()).filter(|t| !t.is_empty())
}

fn valid_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id.split('-').all(|w| {
            !w.is_empty() && w.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

fn check_len(flag: &str, v: &str, min: usize, max: usize) -> Result<(), String> {
    let n = v.chars().count();
    if (min..=max).contains(&n) {
        Ok(())
    } else {
        Err(format!("{flag} must be {min}-{max} characters (got {n})"))
    }
}

pub fn manifest(meta: &PackMeta, pack: &BuiltPack) -> Manifest {
    Manifest {
        format: FORMAT_VERSION,
        id: meta.id.clone(),
        name: meta.name.clone(),
        version: Some(meta.version.clone()).filter(|v| !v.is_empty()),
        author: meta.author.clone(),
        license: meta.license.clone(),
        description: meta.description.clone(),
        source: Some(meta.source.clone()).filter(|s| !s.is_empty()),
        attribution: meta.attribution.clone(),
        preview: Some(PREVIEW_FILE.to_owned()),
        volume: None,
        trim_silence: None,
        variation: None,
        groups: pack.groups.clone(),
        keys: pack.keys.clone(),
        extra: Default::default(),
    }
}

/// Refuses a non-empty output folder unless `force`.
pub fn check_out_dir(dir: &Path, force: bool) -> Result<(), String> {
    match fs::read_dir(dir) {
        Ok(mut entries) => match entries.next() {
            Some(_) if !force => Err(format!(
                "{} is not empty; choose a new folder or pass --force to write into it",
                dir.display()
            )),
            _ => Ok(()),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("{}: {e}", dir.display())),
    }
}

pub struct Written {
    pub files: usize,
    /// Audio files in `sounds/` the new pack does not use (left from an earlier run).
    pub stale: usize,
}

/// Writes `sounds/`, the preview and `pack.json`. Files with the same names are replaced;
/// nothing else is deleted.
pub fn write(
    dir: &Path,
    meta: &PackMeta,
    pack: &BuiltPack,
    force: bool,
) -> Result<Written, String> {
    check_out_dir(dir, force)?;
    let io = |p: &Path, e: std::io::Error| format!("{}: {e}", p.display());
    let sounds = dir.join("sounds");
    fs::create_dir_all(&sounds).map_err(|e| io(&sounds, e))?;
    for s in &pack.sounds {
        wav::write(&dir.join(&s.path), pack.rate, &s.samples)?;
    }
    wav::write(&dir.join(PREVIEW_FILE), pack.rate, &pack.preview)?;
    let mut json =
        serde_json::to_string_pretty(&manifest(meta, pack)).map_err(|e| e.to_string())?;
    json.push('\n');
    let path = dir.join("pack.json");
    fs::write(&path, json).map_err(|e| io(&path, e))?;

    let ours: BTreeSet<&str> = pack.sounds.iter().map(|s| s.path.as_str()).collect();
    let stale = fs::read_dir(&sounds).map_err(|e| io(&sounds, e))?;
    let stale = stale
        .flatten()
        .filter(|e| !ours.contains(format!("sounds/{}", e.file_name().to_string_lossy()).as_str()))
        .count();
    Ok(Written { files: pack.sounds.len() + 2, stale })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::{self, RecordOptions};
    use crate::signal::Audio;
    use crate::slice::{self, SliceOptions};
    use crate::testutil::*;
    use taktak_core::pack::{self, PackOrigin};

    fn meta(license: &str) -> PackMeta {
        PackMeta {
            id: "my-board".into(),
            name: " My Board ".into(),
            author: "Me".into(),
            license: license.into(),
            version: "1.0.0".into(),
            description: None,
            attribution: None,
            source: "Recorded with TakTak pack-maker".into(),
        }
    }

    #[test]
    fn metadata_rules() {
        let mut m = meta("CC0-1.0");
        assert!(m.check().unwrap().is_empty());
        assert_eq!(m.name, "My Board");

        let mut m = meta("CC-BY-4.0");
        assert_eq!(m.check().unwrap().len(), 1);
        assert_eq!(m.attribution.as_deref(), Some("My Board by Me"));

        let mut m = meta(PERSONAL_LICENSE);
        assert_eq!(m.check().unwrap().len(), 1);

        // A blank credit line gets the default, which the core validator requires.
        for blank in ["", "   ", "\t\n"] {
            for lic in ["CC-BY-3.0", "CC-BY-4.0"] {
                let mut m = PackMeta { attribution: Some(blank.into()), ..meta(lic) };
                assert_eq!(m.check().unwrap().len(), 1, "{lic} {blank:?}");
                assert_eq!(m.attribution.as_deref(), Some("My Board by Me"), "{lic} {blank:?}");
            }
            let mut m = PackMeta { attribution: Some(blank.into()), ..meta("MIT") };
            assert!(m.check().unwrap().is_empty());
            assert_eq!(m.attribution, None);
            let mut m = PackMeta { description: Some(blank.into()), ..meta("MIT") };
            m.check().unwrap();
            assert_eq!(m.description, None);
        }
        let mut m = PackMeta { attribution: Some("  Jane Doe ".into()), ..meta("CC-BY-4.0") };
        assert!(m.check().unwrap().is_empty());
        assert_eq!(m.attribution.as_deref(), Some("Jane Doe"));

        for bad in ["My-Board", "my--board", "-a", "a-", "", "a_b", &"a".repeat(65)] {
            let mut m = PackMeta { id: bad.into(), ..meta("MIT") };
            assert!(m.check().is_err(), "{bad:?}");
        }
        for lic in ["CC-BY-NC-4.0", "CC-BY-SA-4.0", "GPL-3.0", "cc0-1.0"] {
            assert!(meta(lic).check().is_err(), "{lic}");
        }
        assert!(PackMeta { name: "  ".into(), ..meta("MIT") }.check().is_err());
        assert!(PackMeta { description: Some("x".repeat(501)), ..meta("MIT") }.check().is_err());
    }

    #[test]
    fn refuses_non_empty_folders_unless_forced() {
        let dir = tempfile::tempdir().unwrap();
        assert!(check_out_dir(&dir.path().join("new"), false).is_ok());
        assert!(check_out_dir(dir.path(), false).is_ok());
        fs::write(dir.path().join("x.txt"), "hi").unwrap();
        assert!(check_out_dir(dir.path(), false).is_err());
        assert!(check_out_dir(dir.path(), true).is_ok());
        assert!(check_out_dir(&dir.path().join("x.txt"), true).is_err(), "a file is not a folder");
    }

    #[test]
    fn recorded_pack_round_trips_through_pack_json() {
        let (audio, marks, _) = session(&typical_session(), -65.0);
        let (built, _) = record::process(&audio, &marks, &RecordOptions::default()).unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("my-board");
        let mut m = meta("CC0-1.0");
        m.check().unwrap();
        let written = write(&dir, &m, &built, false).unwrap();
        assert_eq!((written.files, written.stale), (32, 0));

        let json = fs::read(dir.join("pack.json")).unwrap();
        let back: Manifest = serde_json::from_slice(&json).unwrap();
        assert_eq!(back, manifest(&m, &built));
        assert_eq!(back.format, 1);
        assert_eq!(back.source.as_deref(), Some("Recorded with TakTak pack-maker"));
        assert_eq!(back.preview.as_deref(), Some("preview.wav"));
        assert!(back.extra.is_empty());
        assert_eq!(
            back.keys.keys().map(String::as_str).collect::<Vec<_>>(),
            ["ArrowUp", "KeyA", "KeyS", "ShiftLeft", "Space"]
        );
        assert_eq!(back.keys["KeyA"].press[0], "sounds/KeyA-press-1.wav");
        assert_eq!(
            back.groups.keys().map(String::as_str).collect::<Vec<_>>(),
            ["alphanumeric", "modifiers", "other", "space"]
        );
        assert!(!back.groups["alphanumeric"].press.is_empty(), "fallback press is required");

        // Every referenced file exists and is 16-bit mono at the pack's rate.
        for set in back.keys.values().chain(back.groups.values()) {
            for f in set.press.iter().chain(&set.release) {
                let r = hound::WavReader::open(dir.join(f)).unwrap();
                let spec = r.spec();
                assert_eq!(
                    (spec.channels, spec.bits_per_sample, spec.sample_rate),
                    (1, 16, 48_000)
                );
            }
        }
        let preview = hound::WavReader::open(dir.join("preview.wav")).unwrap();
        assert_eq!(preview.duration(), 72_000);
        // Nothing but the pack is written.
        let mut top: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into())
            .collect();
        top.sort();
        assert_eq!(top, ["pack.json", "preview.wav", "sounds"]);

        // A second run needs --force and reports, but keeps, files the new pack does not use.
        assert!(write(&dir, &m, &built, false).is_err());
        fs::write(dir.join("sounds/old.wav"), b"").unwrap();
        assert_eq!(write(&dir, &m, &built, true).unwrap().stale, 1);
        assert!(dir.join("sounds/old.wav").exists());
    }

    /// Six keystrokes with releases in three seconds of background noise.
    fn typing(rate: u32) -> Audio {
        let mut x = noise(rate as usize * 3, -66.0, 2);
        let step = rate as usize * 5 / 12;
        for i in 0..6 {
            let at = rate as usize * 5 / 24 + i * step;
            add(&mut x, &press_click(rate, 1, 0.4 + i as f32 * 0.01), at);
            add(&mut x, &release_click(rate, 1, 0.15), at + rate as usize * 3 / 32);
        }
        Audio::mono(rate, x)
    }

    #[test]
    fn sliced_pack_round_trips_and_resamples_unusual_rates() {
        // A 96 kHz recording becomes a 48 kHz pack.
        let audio = typing(96_000);
        let (built, stats) = slice::process(&audio, &SliceOptions::default()).unwrap();
        assert_eq!((stats.press_files, stats.release_files), (6, 6));
        let tmp = tempfile::tempdir().unwrap();
        let mut m = PackMeta { source: "Sliced with TakTak pack-maker".into(), ..meta("MIT") };
        m.check().unwrap();
        write(tmp.path(), &m, &built, false).unwrap();
        let back: Manifest =
            serde_json::from_slice(&fs::read(tmp.path().join("pack.json")).unwrap()).unwrap();
        assert!(back.keys.is_empty());
        assert_eq!(back.groups.len(), 1);
        assert_eq!(back.groups["alphanumeric"].press.len(), 6);
        let r = hound::WavReader::open(tmp.path().join("sounds/press-1.wav")).unwrap();
        assert_eq!(r.spec().sample_rate, 48_000);
    }

    /// TakTak itself has the last word on what a pack is: every pack the tool writes must
    /// pass the core validator (strictly, unless personal) and load, so that any drift
    /// between `PackMeta::check` and the core rules fails here instead of after a session.
    #[test]
    fn written_packs_pass_the_core_validator_and_load() {
        let (audio, marks, _) = session(&typical_session(), -65.0);
        let (recorded, _) = record::process(&audio, &marks, &RecordOptions::default()).unwrap();
        let (sliced, _) = slice::process(&typing(48_000), &SliceOptions::default()).unwrap();
        let metas = [
            meta("CC0-1.0"),
            PackMeta { attribution: Some(String::new()), ..meta("CC-BY-4.0") },
            PackMeta {
                attribution: Some("   ".into()),
                description: Some(" ".into()),
                ..meta("CC-BY-3.0")
            },
            PackMeta {
                attribution: Some(" Jane Doe ".into()),
                description: Some(" A quiet board. ".into()),
                version: " 2.0 ".into(),
                source: " ".into(),
                ..meta("MIT")
            },
            meta(PERSONAL_LICENSE),
        ];
        let tmp = tempfile::tempdir().unwrap();
        for (kind, built) in [("record", &recorded), ("slice", &sliced)] {
            for (i, m) in metas.iter().enumerate() {
                let mut m = m.clone();
                m.check().unwrap();
                let dir = tmp.path().join(format!("{kind}-{i}"));
                write(&dir, &m, built, false).unwrap();
                let what = format!("{kind} pack, {}", m.license);
                let personal = m.license == PERSONAL_LICENSE;
                let (info, warnings) = pack::inspect(&dir, PackOrigin::User, !personal)
                    .unwrap_or_else(|e| panic!("{what}: {e}"));
                assert_eq!(info.attribution, m.attribution, "{what}");
                assert_eq!(info.description, m.description, "{what}");
                let expected: &[&str] = if personal { &["license"] } else { &[] };
                let locations: Vec<&str> = warnings.iter().map(|w| w.location.as_str()).collect();
                assert_eq!(locations, expected, "{what}: {warnings:?}");
                let loaded = pack::load(&dir, PackOrigin::User, 48_000)
                    .unwrap_or_else(|e| panic!("{what}: {e}"));
                assert_eq!(loaded.warnings, warnings, "{what}");
                assert!(loaded.preview.is_some(), "{what}");
            }
        }
        let cc_by = pack::inspect(&tmp.path().join("record-1"), PackOrigin::User, true).unwrap();
        assert_eq!(cc_by.0.attribution.as_deref(), Some("My Board by Me"));
        let mit = pack::inspect(&tmp.path().join("slice-3"), PackOrigin::User, true).unwrap();
        assert_eq!(mit.0.attribution.as_deref(), Some("Jane Doe"));
        assert_eq!(mit.0.version.as_deref(), Some("2.0"));
        assert_eq!(mit.0.source, None);
    }
}
