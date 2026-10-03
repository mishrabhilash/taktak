//! Settings persistence: `<app config dir>/settings.json`, written atomically and debounced.
//!
//! A missing file means defaults. A file that is not a JSON object is moved aside to
//! `settings.json.corrupt` and defaults are used. A field with the wrong type falls back to its
//! default on its own, keeping the others; levels are clamped into `0..=1`.
//!
//! Migration (`docs/ui-contract.md` § Settings migration): a file without `onboardingDone` was
//! written by an earlier version, so its user has run TakTak before and the field reads as
//! `true`. Leniency reaches into `appRule`: a bad `mode` or `apps` resets alone, and a bad entry
//! is dropped without losing the others. A `packId` naming a pack TakTak no longer bundles is
//! kept here and moved to the default pack by the service once it knows the installed packs
//! (`catalog::migrate_retired`), so a user pack of that id keeps the selection.

use crate::rules;
use crate::state::{AppRule, AppRuleEntry, AppRuleMode, DEFAULT_PACK_ID, MAX_RULE_APPS, Settings};
use serde_json::{Map, Value};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// The settings file's name inside the app config dir.
pub const FILE_NAME: &str = "settings.json";
/// Quiet time after the last change before the file is written.
pub const DEBOUNCE: Duration = Duration::from_millis(300);
/// Upper bound on the wait while changes keep coming (a long slider drag still gets saved).
const DEBOUNCE_MAX: Duration = Duration::from_secs(2);

/// `value` bounded to `0..=1` (non-finite → 0).
pub fn unit(value: f64) -> f64 {
    if value.is_finite() { value.clamp(0.0, 1.0) } else { 0.0 }
}

/// The engine's master gain for a slider position: its square, so equal slider steps sound
/// like roughly equal loudness steps.
pub fn master_gain(slider: f64) -> f32 {
    let s = unit(slider);
    (s * s) as f32
}

/// `settings` with every level in `0..=1`, a non-empty pack id, no empty hotkey and a clean
/// rule list ([`rules::sanitize`]).
pub fn sanitize(mut settings: Settings) -> Settings {
    settings.master_volume = unit(settings.master_volume);
    settings.press_volume = unit(settings.press_volume);
    settings.release_volume = unit(settings.release_volume);
    settings.humanize = unit(settings.humanize);
    let id = settings.pack_id.trim();
    settings.pack_id = if id.is_empty() { DEFAULT_PACK_ID.to_owned() } else { id.to_owned() };
    if settings.mute_hotkey.as_deref().is_some_and(|h| h.trim().is_empty()) {
        settings.mute_hotkey = None;
    }
    settings.app_rule = rules::sanitize(settings.app_rule);
    settings
}

/// What [`load`] found.
#[derive(Debug, PartialEq)]
pub struct Loaded {
    pub settings: Settings,
    /// Something to log: unreadable or corrupt file, fields that were reset.
    pub note: Option<String>,
}

/// Reads the settings file, recovering from anything wrong with it (see the module docs).
pub fn load(path: &Path) -> Loaded {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Loaded { settings: Settings::default(), note: None };
        }
        Err(e) => {
            return Loaded {
                settings: Settings::default(),
                note: Some(format!("cannot read {}: {e}; using defaults", path.display())),
            };
        }
    };
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(Value::Object(fields)) => {
            let migrated = !fields.contains_key(ONBOARDING_DONE);
            let (mut settings, rejected) = lenient(fields);
            if migrated {
                settings.onboarding_done = true;
            }
            let note = (!rejected.is_empty()).then(|| {
                let fields = rejected.join(", ");
                format!("{}: reset invalid field(s) to defaults: {fields}", path.display())
            });
            Loaded { settings, note }
        }
        _ => {
            let backup = corrupt_backup_path(path);
            let moved = match fs::rename(path, &backup) {
                Ok(()) => format!("moved it to {}", backup.display()),
                Err(e) => format!("could not move it aside: {e}"),
            };
            Loaded {
                settings: Settings::default(),
                note: Some(format!("{} is corrupt ({moved}); using defaults", path.display())),
            }
        }
    }
}

/// Where [`load`] moves a corrupt settings file.
pub fn corrupt_backup_path(path: &Path) -> PathBuf {
    sibling(path, ".corrupt")
}

/// `path` with `suffix` appended to its file name.
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

/// Overlays each known field of `fields` on the defaults, keeping only fields that
/// deserialize; returns the settings and the names of the fields that did not.
fn lenient(fields: Map<String, Value>) -> (Settings, Vec<String>) {
    let Ok(Value::Object(defaults)) = serde_json::to_value(Settings::default()) else {
        return (Settings::default(), Vec::new());
    };
    let mut merged = defaults.clone();
    let mut rejected = Vec::new();
    for (key, value) in fields {
        // Unknown fields (from a newer version, or typos) are ignored, not reported.
        if !defaults.contains_key(&key) {
            continue;
        }
        if key == APP_RULE {
            let (rule, problems) = lenient_app_rule(&value);
            rejected.extend(problems);
            if let Ok(value) = serde_json::to_value(rule) {
                merged.insert(key, value);
            }
            continue;
        }
        let mut probe = defaults.clone();
        probe.insert(key.clone(), value.clone());
        if serde_json::from_value::<Settings>(Value::Object(probe)).is_ok() {
            merged.insert(key, value);
        } else {
            rejected.push(key);
        }
    }
    let settings = serde_json::from_value(Value::Object(merged)).unwrap_or_default();
    (sanitize(settings), rejected)
}

/// The `onboardingDone` field's name in the file.
const ONBOARDING_DONE: &str = "onboardingDone";
/// The `appRule` field's name in the file.
const APP_RULE: &str = "appRule";

/// Reads `appRule` leniently: a wrong-typed `mode` becomes "everywhere" and keeps the list; a
/// wrong-typed `apps` becomes empty; an entry that is not an object with a non-empty string `id`
/// is dropped, the others kept; a missing, wrong-typed or empty `name` becomes the id; entries
/// past the 200th are dropped. Unknown fields are ignored. Returns the rule (sanitized: ids
/// trimmed, later duplicates dropped) and what had to be reset or dropped, for the log.
fn lenient_app_rule(value: &Value) -> (AppRule, Vec<String>) {
    let Value::Object(fields) = value else {
        return (AppRule::default(), vec![APP_RULE.to_owned()]);
    };
    let mut problems = Vec::new();
    let mode = match fields.get("mode") {
        None => AppRuleMode::default(),
        Some(mode) => serde_json::from_value(mode.clone()).unwrap_or_else(|_| {
            problems.push(format!("{APP_RULE}.mode"));
            AppRuleMode::default()
        }),
    };
    let apps = match fields.get("apps") {
        None => Vec::new(),
        Some(Value::Array(entries)) => {
            let apps: Vec<AppRuleEntry> = entries.iter().filter_map(lenient_rule_entry).collect();
            let invalid = entries.len() - apps.len();
            if invalid > 0 {
                problems.push(format!("{APP_RULE}.apps ({invalid} invalid entries dropped)"));
            }
            apps
        }
        Some(_) => {
            problems.push(format!("{APP_RULE}.apps"));
            Vec::new()
        }
    };
    let valid = apps.len();
    let rule = rules::sanitize(AppRule { mode, apps });
    let extra = valid - rule.apps.len();
    if extra > 0 {
        problems.push(format!(
            "{APP_RULE}.apps ({extra} duplicate entries, or entries past the {MAX_RULE_APPS}th, \
             dropped)"
        ));
    }
    (rule, problems)
}

/// One `appRule.apps` entry, if it is an object with a non-empty string `id`.
fn lenient_rule_entry(entry: &Value) -> Option<AppRuleEntry> {
    let id = entry.get("id")?.as_str()?.trim();
    if id.is_empty() {
        return None;
    }
    let name = entry.get("name").and_then(Value::as_str).map(str::trim).unwrap_or_default();
    let name = if name.is_empty() { id } else { name };
    Some(AppRuleEntry { id: id.to_owned(), name: name.to_owned() })
}

/// Writes `settings` to `path` atomically: a temporary file in the same folder, flushed to
/// disk, then renamed over the old file. Creates the folder if needed.
pub fn save(path: &Path, settings: &Settings) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut json = serde_json::to_vec_pretty(settings).map_err(io::Error::other)?;
    json.push(b'\n');
    let tmp = sibling(path, ".tmp");
    let written = (|| {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(&json)?;
        file.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written
}

enum PersistMsg {
    Save(Settings),
    Flush(Sender<()>),
    Stop,
}

/// Cloneable, non-blocking handle for queueing a save.
#[derive(Clone)]
pub struct PersistHandle(Sender<PersistMsg>);

impl PersistHandle {
    /// Queues `settings` to be written once changes have been quiet for the debounce time.
    pub fn save(&self, settings: Settings) {
        let _ = self.0.send(PersistMsg::Save(settings));
    }
}

/// The settings writer thread (`taktak-settings`). Dropping it writes anything pending.
pub struct Persister {
    handle: PersistHandle,
    thread: Option<JoinHandle<()>>,
}

impl Persister {
    /// Starts the writer for `path`, treating `saved` as what the file already holds.
    pub fn start(path: PathBuf, saved: Settings, debounce: Duration) -> io::Result<Persister> {
        let (tx, rx) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("taktak-settings".into())
            .spawn(move || write_loop(&path, saved, debounce, rx))?;
        Ok(Persister { handle: PersistHandle(tx), thread: Some(thread) })
    }

    pub fn handle(&self) -> PersistHandle {
        self.handle.clone()
    }

    /// Writes anything pending now and waits up to `timeout` for it. `false` on timeout.
    pub fn flush(&self, timeout: Duration) -> bool {
        let (ack, done) = mpsc::channel();
        if self.handle.0.send(PersistMsg::Flush(ack)).is_err() {
            return false;
        }
        done.recv_timeout(timeout).is_ok()
    }
}

impl Drop for Persister {
    fn drop(&mut self) {
        let _ = self.handle.0.send(PersistMsg::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Collects saves until they have been quiet for `debounce` (at most [`DEBOUNCE_MAX`] after
/// the first), then writes the latest if it differs from what is on disk. Blocks while idle.
fn write_loop(path: &Path, mut on_disk: Settings, debounce: Duration, rx: Receiver<PersistMsg>) {
    let mut pending: Option<Settings> = None;
    let mut first_change: Option<Instant> = None;
    let mut write = |pending: &mut Option<Settings>| {
        let Some(settings) = pending.take() else { return };
        if settings == on_disk {
            return;
        }
        match save(path, &settings) {
            Ok(()) => on_disk = settings,
            Err(e) => log::warn!("cannot save settings to {}: {e}", path.display()),
        }
    };
    loop {
        let msg = match first_change {
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            Some(first) => {
                rx.recv_timeout(debounce.min(DEBOUNCE_MAX.saturating_sub(first.elapsed())))
            }
        };
        match msg {
            Ok(PersistMsg::Save(settings)) => {
                first_change.get_or_insert_with(Instant::now);
                pending = Some(settings);
            }
            Ok(PersistMsg::Flush(ack)) => {
                write(&mut pending);
                first_change = None;
                let _ = ack.send(());
            }
            Err(RecvTimeoutError::Timeout) => {
                write(&mut pending);
                first_change = None;
            }
            Ok(PersistMsg::Stop) | Err(RecvTimeoutError::Disconnected) => {
                write(&mut pending);
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::VariantMode;

    #[test]
    fn unit_clamps() {
        assert_eq!(unit(-0.5), 0.0);
        assert_eq!(unit(0.25), 0.25);
        assert_eq!(unit(7.0), 1.0);
        assert_eq!(unit(f64::NAN), 0.0);
        assert_eq!(unit(f64::INFINITY), 0.0);
    }

    #[test]
    fn volume_curve_is_the_square_of_the_slider() {
        assert_eq!(master_gain(0.0), 0.0);
        assert_eq!(master_gain(1.0), 1.0);
        assert!((master_gain(0.7) - 0.49).abs() < 1e-6);
        assert!((master_gain(0.5) - 0.25).abs() < 1e-6);
        assert_eq!(master_gain(3.0), 1.0);
        assert_eq!(master_gain(-1.0), 0.0);
        assert_eq!(master_gain(f64::NAN), 0.0);
    }

    #[test]
    fn missing_file_gives_defaults_without_a_note() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = load(&dir.path().join(FILE_NAME));
        assert_eq!(loaded, Loaded { settings: Settings::default(), note: None });
    }

    #[test]
    fn save_then_load_round_trips_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join(FILE_NAME);
        let settings = Settings {
            enabled: false,
            pack_id: "typewriter".into(),
            master_volume: 0.33,
            variant_mode: VariantMode::Random,
            mute_hotkey: None,
            launch_at_login: true,
            ..Settings::default()
        };
        save(&path, &settings).unwrap();
        assert_eq!(load(&path), Loaded { settings: settings.clone(), note: None });
        // No temporary file is left behind, and saving again replaces the file.
        let names: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names, [FILE_NAME]);
        save(&path, &Settings::default()).unwrap();
        assert_eq!(load(&path).settings, Settings::default());
    }

    #[test]
    fn corrupt_file_is_moved_aside_and_defaults_are_used() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        for junk in ["{ not json", "[1, 2]", "\"text\"", ""] {
            fs::write(&path, junk).unwrap();
            let loaded = load(&path);
            assert_eq!(loaded.settings, Settings::default(), "{junk:?}");
            assert!(loaded.note.unwrap().contains("corrupt"), "{junk:?}");
            assert!(!path.exists());
            assert_eq!(fs::read_to_string(corrupt_backup_path(&path)).unwrap(), junk);
        }
    }

    #[test]
    fn bad_fields_reset_alone_and_levels_are_clamped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        fs::write(
            &path,
            r#"{
                "enabled": "yes",
                "packId": "typewriter",
                "masterVolume": 4.5,
                "pressVolume": -1,
                "releaseVolume": 0.5,
                "variantMode": "chaotic",
                "humanize": 0.1,
                "muteHotkey": "  ",
                "launchAtLogin": true,
                "fromTheFuture": 1
            }"#,
        )
        .unwrap();
        let loaded = load(&path);
        let s = loaded.settings;
        assert!(s.enabled, "wrong type → default");
        assert_eq!(s.pack_id, "typewriter");
        assert_eq!((s.master_volume, s.press_volume, s.release_volume), (1.0, 0.0, 0.5));
        assert_eq!(s.variant_mode, VariantMode::Consistent);
        assert_eq!(s.humanize, 0.1);
        assert_eq!(s.mute_hotkey, None);
        assert!(s.launch_at_login);
        let note = loaded.note.unwrap();
        assert!(note.contains("enabled") && note.contains("variantMode"), "{note}");
        assert!(!note.contains("fromTheFuture"), "{note}");
        // A repaired file is not moved aside.
        assert!(path.exists());
    }

    #[test]
    fn sanitize_fixes_empty_ids() {
        let s = sanitize(Settings { pack_id: "  ".into(), ..Settings::default() });
        assert_eq!(s.pack_id, DEFAULT_PACK_ID);
        let s = sanitize(Settings { pack_id: " linear-red ".into(), ..Settings::default() });
        assert_eq!(s.pack_id, "linear-red");
    }

    /// A Milestone 3 file: the new fields take their defaults, except `onboardingDone`, which
    /// is true (whoever has a settings file has run TakTak before).
    #[test]
    fn milestone_3_files_migrate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        fs::write(
            &path,
            r#"{
                "enabled": true,
                "packId": "typewriter",
                "masterVolume": 0.5,
                "pressVolume": 1.0,
                "releaseVolume": 0.8,
                "variantMode": "random",
                "humanize": 0.25,
                "muteHotkey": "CommandOrControl+Alt+Shift+M",
                "launchAtLogin": false
            }"#,
        )
        .unwrap();
        let loaded = load(&path);
        assert_eq!(loaded.note, None, "migrating is not a problem");
        let s = loaded.settings;
        assert_eq!((s.pack_id.as_str(), s.master_volume), ("typewriter", 0.5));
        assert_eq!(s.app_rule, AppRule::default());
        assert!(!s.mute_on_output_change);
        assert!(s.onboarding_done);

        // Written back, the field is there, and false stays false.
        save(&path, &Settings { onboarding_done: false, ..s }).unwrap();
        assert!(!load(&path).settings.onboarding_done);
        // A fresh install (no file) has not seen the onboarding.
        assert!(!load(&dir.path().join("missing.json")).settings.onboarding_done);
        // Even an almost empty file was written by TakTak.
        fs::write(&path, "{}").unwrap();
        assert!(load(&path).settings.onboarding_done);
        // A wrong-typed value resets to the default, like any other field.
        fs::write(&path, r#"{"onboardingDone": "yes"}"#).unwrap();
        let loaded = load(&path);
        assert!(!loaded.settings.onboarding_done);
        assert!(loaded.note.unwrap().contains("onboardingDone"));
    }

    #[test]
    fn app_rules_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        let settings = Settings {
            app_rule: AppRule {
                mode: AppRuleMode::Only,
                apps: vec![
                    AppRuleEntry { id: "com.apple.Safari".into(), name: "Safari".into() },
                    AppRuleEntry { id: "us.zoom.xos".into(), name: "zoom.us".into() },
                ],
            },
            mute_on_output_change: true,
            onboarding_done: true,
            ..Settings::default()
        };
        save(&path, &settings).unwrap();
        assert_eq!(load(&path), Loaded { settings, note: None });
    }

    fn app_rule_of(json: &str) -> (AppRule, Option<String>) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        fs::write(&path, format!(r#"{{"packId": "typewriter", "appRule": {json}}}"#)).unwrap();
        let loaded = load(&path);
        assert_eq!(loaded.settings.pack_id, "typewriter", "other fields are kept");
        (loaded.settings.app_rule, loaded.note)
    }

    fn entry(id: &str, name: &str) -> AppRuleEntry {
        AppRuleEntry { id: id.into(), name: name.into() }
    }

    #[test]
    fn app_rule_leniency_keeps_what_it_can() {
        // A wrong-typed mode keeps the list.
        let (rule, note) =
            app_rule_of(r#"{"mode": "sometimes", "apps": [{"id": "a.b", "name": "A"}]}"#);
        assert_eq!(rule, AppRule { mode: AppRuleMode::Everywhere, apps: vec![entry("a.b", "A")] });
        assert!(note.unwrap().contains("appRule.mode"));

        // A wrong-typed list becomes empty and keeps the mode.
        let (rule, note) = app_rule_of(r#"{"mode": "never", "apps": {"id": "a.b"}}"#);
        assert_eq!(rule, AppRule { mode: AppRuleMode::Never, apps: vec![] });
        assert!(note.unwrap().contains("appRule.apps"));

        // A wrong-typed appRule resets as a whole.
        let (rule, note) = app_rule_of(r#""only""#);
        assert_eq!(rule, AppRule::default());
        assert!(note.unwrap().contains("appRule"));

        // Bad entries are dropped, the others kept; ids trimmed; names defaulted; duplicates
        // dropped; unknown fields ignored (and not written back).
        let (rule, note) = app_rule_of(
            r#"{
                "mode": "only",
                "future": true,
                "apps": [
                    {"id": " com.apple.Safari ", "name": " Safari ", "packId": "typewriter"},
                    "com.bare.string",
                    {"name": "No id"},
                    {"id": 42, "name": "Number"},
                    {"id": "   ", "name": "Blank"},
                    {"id": "us.zoom.xos"},
                    {"id": "com.hnc.Discord", "name": 7},
                    {"id": "com.tinyspeck.slackmacgap", "name": ""},
                    {"id": "com.apple.Safari", "name": "Duplicate"},
                    null
                ]
            }"#,
        );
        assert_eq!(
            rule,
            AppRule {
                mode: AppRuleMode::Only,
                apps: vec![
                    entry("com.apple.Safari", "Safari"),
                    entry("us.zoom.xos", "us.zoom.xos"),
                    entry("com.hnc.Discord", "com.hnc.Discord"),
                    entry("com.tinyspeck.slackmacgap", "com.tinyspeck.slackmacgap"),
                ],
            }
        );
        let note = note.unwrap();
        assert!(note.contains("5 invalid entries") && note.contains("1 duplicate"), "{note}");

        // Missing parts take their defaults without a note.
        let (rule, note) = app_rule_of(r#"{"apps": [{"id": "a.b", "name": "A"}]}"#);
        assert_eq!(rule.mode, AppRuleMode::Everywhere);
        assert_eq!(rule.apps, [entry("a.b", "A")]);
        assert_eq!(note, None);
        let (rule, note) = app_rule_of(r#"{"mode": "never"}"#);
        assert_eq!(rule, AppRule { mode: AppRuleMode::Never, apps: vec![] });
        assert_eq!(note, None);
    }

    #[test]
    fn at_most_two_hundred_listed_apps_load() {
        let apps: Vec<String> =
            (0..250).map(|i| format!(r#"{{"id": "com.example.app{i}", "name": "App"}}"#)).collect();
        let (rule, note) =
            app_rule_of(&format!(r#"{{"mode": "never", "apps": [{}]}}"#, apps.join(",")));
        assert_eq!(rule.apps.len(), MAX_RULE_APPS);
        assert_eq!(rule.apps[199].id, "com.example.app199");
        assert!(note.unwrap().contains("50 duplicate entries, or entries past the 200th"));
    }

    /// Counts the writes the persister makes by watching the file's content change.
    #[test]
    fn persister_debounces_and_flushes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        let debounce = Duration::from_millis(40);
        let persister = Persister::start(path.clone(), Settings::default(), debounce).unwrap();
        let handle = persister.handle();

        // Unchanged settings are never written.
        handle.save(Settings::default());
        assert!(persister.flush(Duration::from_secs(2)));
        assert!(!path.exists());

        // A burst of changes ends up as the last one, once things are quiet.
        for i in 1..=20 {
            handle.save(Settings { master_volume: f64::from(i) / 20.0, ..Settings::default() });
        }
        assert!(!path.exists(), "written before the debounce time");
        let deadline = Instant::now() + Duration::from_secs(5);
        while !path.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(load(&path).settings.master_volume, 1.0);

        // Flush writes immediately; dropping writes whatever is still pending.
        handle.save(Settings { humanize: 0.5, ..Settings::default() });
        assert!(persister.flush(Duration::from_secs(2)));
        assert_eq!(load(&path).settings.humanize, 0.5);
        handle.save(Settings { humanize: 0.75, ..Settings::default() });
        drop(persister);
        assert_eq!(load(&path).settings.humanize, 0.75);
    }
}
