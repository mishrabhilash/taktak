//! "Import Mechvibes pack…" (M5): the native picker, the import itself on a worker thread, and
//! what the UI is told (`docs/ui-contract.md` § Importing Mechvibes packs).
//!
//! The import is [`taktak_core::pack::import::import_mechvibes`]: it writes the converted pack
//! to a hidden folder inside the user packs folder, validates it, then moves it into place, so
//! the registry's hot reload lists it within about a second and never sees half a pack. Imported
//! packs are `LicenseRef-Personal`: they play here and are never bundled or shared.
//!
//! The UI never hands TakTak a path: the source always comes from the native picker, and
//! `overwrite_mechvibes_pack` reuses the source of the last import that found the pack already
//! imported, kept here in memory only.
//!
//! Privacy: only pack contents (file names, key names from the pack's config) are reported, and
//! nothing about the source is logged.

use crate::state::{ImportSummary, MechvibesImport};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use taktak_core::pack::import::{ImportError, ImportOptions, ImportReport, import_mechvibes};
use taktak_core::pack::printable;

/// The most lines `ImportSummary::warnings` carries; the last one then says how many more.
pub const MAX_WARNINGS: usize = 30;
/// Rejection while a picker is open or an import runs.
pub const BUSY: &str = "TakTak is already importing a pack, or its pack chooser is still open.";
/// Rejection of `overwrite_mechvibes_pack` with nothing waiting to be replaced.
pub const NOTHING_TO_OVERWRITE: &str =
    "There is no import waiting to be replaced. Choose the pack again with Import Mechvibes pack…";
/// Rejection when this system has no user packs folder.
pub const NO_USER_DIR: &str = "This system has no folder for your own packs.";

/// Set while a picker is open or an import runs: one at a time.
static BUSY_FLAG: AtomicBool = AtomicBool::new(false);
/// The source of the last import that found the pack already imported.
static PENDING: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Holds the one import slot; released on drop, whatever happens.
pub struct Busy(());

impl Busy {
    /// Takes the import slot, or rejects with [`BUSY`].
    pub fn take() -> Result<Busy, String> {
        if BUSY_FLAG.swap(true, Ordering::SeqCst) { Err(BUSY.to_owned()) } else { Ok(Busy(())) }
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        BUSY_FLAG.store(false, Ordering::SeqCst);
    }
}

fn pending() -> std::sync::MutexGuard<'static, Option<PathBuf>> {
    PENDING.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Takes the source an `AlreadyImported` outcome left for `overwrite_mechvibes_pack`.
pub fn take_pending() -> Option<PathBuf> {
    pending().take()
}

/// What the UI is shown for a finished import.
pub fn summary(report: &ImportReport) -> ImportSummary {
    let p = |s: &str| printable(s).into_owned();
    let mut warnings: Vec<String> = Vec::new();
    for k in &report.skipped {
        warnings.push(format!("Skipped key “{}”: {}", p(&k.key), p(&k.reason)));
    }
    for file in &report.missing_files {
        warnings.push(format!("Missing file: {}", p(file)));
    }
    for (file, reason) in &report.unreadable_files {
        warnings.push(format!("Couldn’t read {}: {}", p(file), p(reason)));
    }
    warnings.extend(report.warnings.iter().map(|w| p(w)));
    // The loader's note on the personal license: the UI explains that with the Personal badge.
    warnings
        .extend(report.pack_warnings.iter().filter(|w| !w.starts_with("license:")).map(|w| p(w)));
    if warnings.len() > MAX_WARNINGS {
        let more = warnings.len() - (MAX_WARNINGS - 1);
        warnings.truncate(MAX_WARNINGS - 1);
        warnings.push(format!("…and {more} more."));
    }
    ImportSummary {
        id: report.id.clone(),
        name: p(&report.name),
        source: p(&report.source),
        format: report.format.to_string(),
        keys_mapped: report.keys_mapped.len(),
        keys_with_release: report.keys_with_release,
        sounds_written: report.sounds_written,
        replaced: report.replaced,
        warnings,
    }
}

/// The user-facing message for an import that failed (everything but `AlreadyImported`, which
/// is an outcome, not an error).
pub fn error_message(error: &ImportError) -> String {
    let p = |s: &str| printable(s).into_owned();
    match error {
        ImportError::Io { path, message } => {
            format!("Could not read or write {}: {}.", p(&path.display().to_string()), p(message))
        }
        ImportError::NotAPack(why) => format!(
            "That isn’t a Mechvibes pack ({}). Choose the pack’s folder, the one with \
             config.json in it, or its .zip.",
            p(why)
        ),
        ImportError::InvalidConfig(why) => {
            format!("The pack’s config.json can’t be read: {}.", p(why))
        }
        ImportError::Unsupported(why) => format!(
            "TakTak can’t import this pack: {}. It imports Mechvibes keyboard packs (v1 single \
             and multi, v2, Mechvibes++ and MechvibesDX).",
            p(why)
        ),
        ImportError::NoSounds(why) => format!(
            "No usable sounds in this pack: {}. Check that the folder or .zip still has the \
             sound files its config.json names, in a format TakTak reads (WAV, Ogg Vorbis or \
             MP3).",
            p(why)
        ),
        ImportError::AlreadyImported { id, .. } => {
            format!("This pack was imported before, as {}.", p(id))
        }
        ImportError::Invalid(e) => format!(
            "The converted pack did not pass TakTak’s checks, so nothing was installed (this is \
             a bug in TakTak): {}",
            p(&e.to_string())
        ),
    }
}

/// The source's file name, for the UI.
fn source_name(src: &Path) -> String {
    src.file_name()
        .map(|n| printable(&n.to_string_lossy()).into_owned())
        .unwrap_or_else(|| printable(&src.display().to_string()).into_owned())
}

/// Imports `src` into `user_dir` (created if needed). Blocks for as long as decoding and
/// writing take (up to a few seconds for a large pack): call it on a worker thread, never on
/// the main thread. An `AlreadyImported` outcome remembers `src` for [`take_pending`].
pub fn import(src: &Path, user_dir: &Path, overwrite: bool) -> Result<MechvibesImport, String> {
    std::fs::create_dir_all(user_dir).map_err(|e| {
        format!("Could not create {}: {e}.", printable(&user_dir.display().to_string()))
    })?;
    let options = ImportOptions { overwrite, ..ImportOptions::default() };
    match import_mechvibes(src, user_dir, options) {
        Ok(report) => {
            *pending() = None;
            log::info!(
                "imported a Mechvibes pack as {} ({} keys{})",
                report.id,
                report.keys_mapped.len(),
                if report.replaced { ", replacing the earlier import" } else { "" }
            );
            Ok(MechvibesImport::Imported { pack: summary(&report) })
        }
        Err(ImportError::AlreadyImported { id, .. }) => {
            *pending() = Some(src.to_path_buf());
            Ok(MechvibesImport::AlreadyImported {
                id: printable(&id).into_owned(),
                source: source_name(src),
            })
        }
        Err(e) => {
            *pending() = None;
            log::warn!("a Mechvibes import failed: {}", error_variant(&e));
            Err(error_message(&e))
        }
    }
}

/// The kind of failure, for the log (no paths, no pack contents).
fn error_variant(error: &ImportError) -> &'static str {
    match error {
        ImportError::Io { .. } => "file error",
        ImportError::NotAPack(_) => "not a Mechvibes pack",
        ImportError::InvalidConfig(_) => "invalid config",
        ImportError::Unsupported(_) => "unsupported pack",
        ImportError::NoSounds(_) => "no usable sounds",
        ImportError::AlreadyImported { .. } => "already imported",
        ImportError::Invalid(_) => "converted pack invalid",
    }
}

// --- the picker -------------------------------------------------------------------------------

/// What the picker hands back: the chosen folder or `.zip`, `None` when cancelled, or a
/// message for the user.
pub type Picked = Result<Option<PathBuf>, String>;

#[cfg(target_os = "macos")]
pub use mac::pick;

#[cfg(target_os = "macos")]
mod mac {
    use super::Picked;
    use block2::RcBlock;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSModalResponse, NSModalResponseOK, NSOpenPanel};
    use objc2_foundation::{NSArray, NSString};
    use std::cell::Cell;

    /// Opens one panel that takes a pack folder or a `.zip` and calls `done` when it closes (on
    /// the main thread). Returns at once. Main thread only.
    pub fn pick(done: Box<dyn FnOnce(Picked) + Send>) {
        let Some(mtm) = MainThreadMarker::new() else {
            done(Err("The pack chooser could not be opened.".to_owned()));
            return;
        };
        let panel = NSOpenPanel::openPanel(mtm);
        panel.setCanChooseFiles(true);
        panel.setCanChooseDirectories(true);
        panel.setAllowsMultipleSelection(false);
        panel.setResolvesAliases(true);
        let types = NSArray::from_retained_slice(&[
            NSString::from_str("zip"),
            NSString::from_str("public.zip-archive"),
            NSString::from_str("public.folder"),
        ]);
        // `allowedContentTypes` needs UniformTypeIdentifiers; extensions and UTIs work here too.
        #[allow(deprecated)]
        panel.setAllowedFileTypes(Some(&types));
        panel.setPrompt(Some(&NSString::from_str("Import")));
        panel.setMessage(Some(&NSString::from_str(
            "Choose a Mechvibes pack: its folder (the one with config.json) or its .zip.",
        )));
        // An accessory app is not activated by showing a window; bring the picker forward.
        #[allow(deprecated)]
        NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);

        let done = Cell::new(Some(done));
        let chosen = panel.clone();
        let handler = RcBlock::new(move |response: NSModalResponse| {
            let result = if response == NSModalResponseOK {
                match chosen.URL().map(|url| url.to_file_path()) {
                    Some(Some(path)) => Ok(Some(path)),
                    Some(None) => Err("TakTak can only import packs stored on this Mac.".into()),
                    None => Ok(None),
                }
            } else {
                Ok(None)
            };
            if let Some(done) = done.take() {
                done(result);
            }
        });
        panel.beginWithCompletionHandler(&handler);
    }
}

/// The platform picker (tauri-plugin-dialog): a folder picker for [`crate::state::PickKind::Folder`], a
/// `.zip` picker otherwise. Blocks until it closes: call it on a worker thread.
#[cfg(not(target_os = "macos"))]
pub fn pick_blocking<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    kind: crate::state::PickKind,
) -> Picked {
    use crate::state::PickKind;
    use tauri::Manager;
    use tauri_plugin_dialog::DialogExt;

    let mut dialog = app.dialog().file().set_title("Import a Mechvibes pack");
    if let Some(settings) = app.get_webview_window(crate::windows::SETTINGS) {
        dialog = dialog.set_parent(&settings);
    }
    let picked = match kind {
        PickKind::Folder => dialog.blocking_pick_folder(),
        PickKind::Any | PickKind::Zip => {
            dialog.add_filter("Mechvibes pack (.zip)", &["zip"]).blocking_pick_file()
        }
    };
    picked
        .map(|path| {
            path.into_path()
                .map_err(|_| "TakTak can only import packs stored on this computer.".into())
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use taktak_core::pack::import::{SkippedKey, SourceFormat, SplitSummary};

    fn report() -> ImportReport {
        ImportReport {
            id: "mv-cream".into(),
            name: "Cream\u{1b}[2J".into(),
            path: PathBuf::from("/packs/mv-cream"),
            source: "cream.zip".into(),
            format: SourceFormat::MechvibesV2,
            sample_rate: 44_100,
            keys_mapped: vec!["KeyA".into(), "Space".into()],
            keys_with_release: 1,
            split: SplitSummary::default(),
            skipped: vec![SkippedKey { key: "999".into(), reason: "unknown key code".into() }],
            missing_files: vec!["gone.wav".into()],
            unreadable_files: vec![("bad.ogg".into(), "not audio".into())],
            warnings: vec!["the config names 3 sounds for Enter; using the first".into()],
            sounds_written: 3,
            pack_warnings: vec![
                "license: LicenseRef-Personal: this pack is for personal use only; do not share it"
                    .into(),
                "sounds/space.wav: longer than 1 s".into(),
            ],
            replaced: false,
        }
    }

    #[test]
    fn summaries_count_escape_and_drop_the_license_note() {
        let s = summary(&report());
        assert_eq!(s.id, "mv-cream");
        assert_eq!(s.name, "Cream\\u{1b}[2J");
        assert_eq!(s.format, "Mechvibes v2");
        assert_eq!((s.keys_mapped, s.keys_with_release, s.sounds_written), (2, 1, 3));
        assert_eq!(
            s.warnings,
            [
                "Skipped key “999”: unknown key code",
                "Missing file: gone.wav",
                "Couldn’t read bad.ogg: not audio",
                "the config names 3 sounds for Enter; using the first",
                "sounds/space.wav: longer than 1 s",
            ]
        );
    }

    #[test]
    fn long_warning_lists_are_capped() {
        let mut r = report();
        r.skipped = (0..100)
            .map(|i| SkippedKey { key: i.to_string(), reason: "unknown key code".into() })
            .collect();
        let s = summary(&r);
        assert_eq!(s.warnings.len(), MAX_WARNINGS);
        // 100 skipped + 4 others, 29 shown.
        assert_eq!(s.warnings.last().unwrap(), "…and 75 more.");
    }

    #[test]
    fn errors_say_what_to_do() {
        let no_sounds = error_message(&ImportError::NoSounds("every file is missing".into()));
        assert!(no_sounds.contains("every file is missing") && no_sounds.contains("config.json"));
        let unsupported = error_message(&ImportError::Unsupported("a mouse pack".into()));
        assert!(unsupported.contains("a mouse pack") && unsupported.contains("MechvibesDX"));
        let not_a_pack = error_message(&ImportError::NotAPack("no config.json".into()));
        assert!(not_a_pack.contains(".zip"));
        assert!(error_message(&ImportError::InvalidConfig("x\u{7}".into())).contains("x\\u{7}"));
    }

    #[test]
    fn one_import_at_a_time() {
        let first = Busy::take().unwrap();
        assert_eq!(Busy::take().err().as_deref(), Some(BUSY));
        drop(first);
        drop(Busy::take().unwrap());
    }

    #[test]
    fn already_imported_packs_wait_for_an_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("Cream");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(
            src.join("config.json"),
            r#"{"name": "Cream", "key_define_type": "multi", "defines": {"30": "a.wav"}}"#,
        )
        .unwrap();
        std::fs::write(src.join("a.wav"), crate::selftest::click_wav(44_100, 1_000)).unwrap();
        let packs = dir.path().join("packs");

        let first = import(&src, &packs, false).unwrap();
        let MechvibesImport::Imported { pack } = first else { panic!("{first:?}") };
        assert_eq!((pack.id.as_str(), pack.keys_mapped, pack.replaced), ("mv-cream", 1, false));
        assert!(take_pending().is_none());

        let again = import(&src, &packs, false).unwrap();
        assert_eq!(
            again,
            MechvibesImport::AlreadyImported { id: "mv-cream".into(), source: "Cream".into() }
        );
        let src_again = take_pending().expect("kept for the overwrite");
        let replaced = import(&src_again, &packs, true).unwrap();
        assert!(matches!(replaced, MechvibesImport::Imported { pack } if pack.replaced));

        let broken = dir.path().join("Broken");
        std::fs::create_dir_all(&broken).unwrap();
        let err = import(&broken, &packs, false).unwrap_err();
        assert!(err.contains("isn’t a Mechvibes pack"), "{err}");
    }
}
