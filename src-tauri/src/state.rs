//! The state the UI renders, mirroring `docs/ui-contract.md` field for field.
//!
//! Every type serializes with camelCase field names and lowercase enum values, exactly as the
//! TypeScript types in `src/lib/types.ts`. Nothing here ever holds a key identity or typed text.

use crate::automute::AutoMuteReasons;
use serde::{Deserialize, Serialize};
use taktak_core::audio::{DEFAULT_HUMANIZE, VariantMode as CoreVariantMode};
use taktak_core::pack::PackOrigin as CorePackOrigin;

/// The pack selected when nothing else is (fresh install, saved pack gone).
pub const DEFAULT_PACK_ID: &str = "buckling-spring";
/// Packs earlier versions bundled (and selected by default) that TakTak no longer ships. A
/// saved selection of one of them that is not installed (as a user pack) moves to
/// [`DEFAULT_PACK_ID`] silently instead of reporting a missing pack.
pub const RETIRED_PACK_IDS: [&str; 3] = ["deep-thock", "crisp-clack", "blue-click"];
/// The mute hotkey on a fresh install.
pub const DEFAULT_MUTE_HOTKEY: &str = "CommandOrControl+Alt+Shift+M";
/// TakTak's own bundle identifier: never a rule entry, and never reported as `frontmostApp`.
pub const OWN_APP_ID: &str = "tech.taktak.app";
/// The most apps `Settings::app_rule` lists.
pub const MAX_RULE_APPS: usize = 200;

/// How a key picks among its candidate samples.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VariantMode {
    #[default]
    Consistent,
    Random,
}

impl From<VariantMode> for CoreVariantMode {
    fn from(mode: VariantMode) -> CoreVariantMode {
        match mode {
            VariantMode::Consistent => CoreVariantMode::Consistent,
            VariantMode::Random => CoreVariantMode::Random,
        }
    }
}

/// Everything the user chooses; persisted to `<app config dir>/settings.json`.
///
/// Missing fields deserialize to their defaults, so an older or hand-edited file still loads.
/// Levels are `f64` so the UI gets back exactly the numbers it sent; the engine takes `f32`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Master on/off (tray toggle).
    pub enabled: bool,
    /// Active pack id.
    pub pack_id: String,
    /// Slider position in `0..=1`; the engine gain is its square.
    pub master_volume: f64,
    /// Linear, `0..=1`.
    pub press_volume: f64,
    /// Linear, `0..=1`.
    pub release_volume: f64,
    pub variant_mode: VariantMode,
    /// Share of the pack's variation per keystroke, `0..=1`.
    pub humanize: f64,
    /// Tauri accelerator, e.g. `CommandOrControl+Alt+Shift+M`; `None` = no hotkey.
    pub mute_hotkey: Option<String>,
    pub launch_at_login: bool,
    /// Per-app rules (M4).
    pub app_rule: AppRule,
    /// Auto-mute when the default output device changes (M4).
    pub mute_on_output_change: bool,
    /// The onboarding window was closed at least once (M4). `false` here (a fresh install); the
    /// settings loader treats an existing file without the field as `true` (contract § Settings
    /// migration), since whoever has a settings file has run TakTak before.
    pub onboarding_done: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            enabled: true,
            pack_id: DEFAULT_PACK_ID.to_owned(),
            master_volume: 0.7,
            press_volume: 1.0,
            release_volume: 1.0,
            variant_mode: VariantMode::Consistent,
            humanize: f64::from(DEFAULT_HUMANIZE),
            mute_hotkey: Some(DEFAULT_MUTE_HOTKEY.to_owned()),
            launch_at_login: false,
            app_rule: AppRule::default(),
            mute_on_output_change: false,
            onboarding_done: false,
        }
    }
}

/// How per-app rules use the list (M4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppRuleMode {
    /// Sounds in every app; the list is kept but ignored.
    #[default]
    Everywhere,
    /// Sounds only while a listed app is frontmost.
    Only,
    /// Silent while a listed app is frontmost.
    Never,
}

/// One listed app (M4).
///
/// A struct rather than a bare id, so per-app overrides (a pack, a volume) can be added later as
/// `Option` fields with `#[serde(default, skip_serializing_if = "Option::is_none")]`, absent =
/// follow the global setting, without migrating `settings.json`. Unknown fields (from a newer
/// version) are ignored.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppRuleEntry {
    /// macOS bundle identifier; unique within the list.
    pub id: String,
    /// Display name when it was added. Sanitizing replaces an empty one with the id.
    #[serde(default)]
    pub name: String,
}

/// Per-app rules (M4): one list for both `Only` and `Never`, in append order, at most
/// [`MAX_RULE_APPS`] entries with unique ids.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppRule {
    pub mode: AppRuleMode,
    pub apps: Vec<AppRuleEntry>,
}

/// An app as TakTak identifies it (M4): `AppState::frontmost_app`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppRef {
    /// macOS bundle identifier.
    pub id: String,
    /// Localized display name.
    pub name: String,
}

/// A pickable app (M4): `list_running_apps`, `choose_app`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    /// Bundle identifier.
    pub id: String,
    /// Localized display name.
    pub name: String,
    /// `data:image/png;base64,…`, 32 × 32 px (16 pt @2x); `None` = no icon.
    pub icon_data_url: Option<String>,
}

/// Why TakTak muted itself (M4). When both apply, `ScreenLocked` is the one reported.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AutoMute {
    /// The screen is locked or this user session is inactive; clears by itself on unlock.
    ScreenLocked,
    /// The default output device changed while `mute_on_output_change` was on; stays until the
    /// user unmutes, turns sounds on or turns the setting off.
    OutputChanged,
}

/// What the onboarding window needs to know (M4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingStatus {
    /// Offer the onboarding: `!settings.onboarding_done`, or `permission_required` and the
    /// permission is denied (live). At startup the window opens when this is true after the first
    /// permission check.
    pub offer: bool,
    /// The platform needs a permission the user grants (macOS with the key listener on).
    pub permission_required: bool,
    /// The platform reports the permission as granted but the key listener cannot start: a
    /// relaunch usually fixes it (macOS: Input Monitoring; elsewhere the listener failed).
    pub relaunch_suggested: bool,
    /// Linux (M5): the key listener has to read the keyboard devices directly (Wayland, or
    /// `TAKTAK_INPUT=evdev`) and cannot, because the user is not in the `input` group.
    pub input_group_needed: bool,
}

/// Where a pack was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PackOrigin {
    Bundled,
    User,
}

impl From<CorePackOrigin> for PackOrigin {
    fn from(origin: CorePackOrigin) -> PackOrigin {
        match origin {
            CorePackOrigin::Bundled => PackOrigin::Bundled,
            CorePackOrigin::User => PackOrigin::User,
        }
    }
}

/// A loadable pack, as listed in the UI.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackSummary {
    pub id: String,
    pub name: String,
    pub author: String,
    /// SPDX expression.
    pub license: String,
    pub description: Option<String>,
    pub attribution: Option<String>,
    pub origin: PackOrigin,
    /// The pack has any release sounds at all.
    pub has_release: bool,
    /// The pack has per-key entries.
    pub per_key: bool,
    /// Human-readable, already escaped (`pack::printable`).
    pub warnings: Vec<String>,
}

/// A pack folder or zip that failed validation.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InvalidPack {
    /// Path shown to the user.
    pub location: String,
    /// Formatted `Problem` lines.
    pub problems: Vec<String>,
}

/// Input Monitoring (macOS) or the platform equivalent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Permission {
    Granted,
    Denied,
    #[default]
    Unknown,
}

/// The output stream's health.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioState {
    Ok,
    #[default]
    Starting,
    Fault,
}

/// The output device and stream, for the settings window.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioStatus {
    pub device: Option<String>,
    pub sample_rate: Option<u32>,
    pub buffer_frames: Option<u32>,
    pub state: AudioState,
    /// User-facing, e.g. "Output device disconnected — reconnecting…".
    pub message: Option<String>,
}

/// Everything the UI renders; sent whole by every command and every `state-changed` event.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    pub version: String,
    pub settings: Settings,
    /// The manual mute (hotkey, tray, `set_muted`), separate from `settings.enabled`. Auto-mute
    /// never changes it.
    pub muted: bool,
    /// A key press makes a sound now: `enabled && !muted && auto_mute.is_none() &&
    /// !rule_blocked && permission granted && audio ok`.
    pub playing: bool,
    /// Sorted by name.
    pub packs: Vec<PackSummary>,
    pub invalid_packs: Vec<InvalidPack>,
    /// The pack whose sounds are loaded: the selected one, a fallback, or the selected pack's
    /// last working version. `None` = the built-in click (also before the first load).
    pub playing_pack_id: Option<String>,
    /// Set when the selected pack is not the one playing: why, and what plays instead.
    pub active_pack_error: Option<String>,
    /// Set when the saved mute hotkey could not be registered (invalid, or taken by another
    /// app): it does nothing until it is changed. Cleared by a successful `set_mute_hotkey`.
    pub mute_hotkey_error: Option<String>,
    pub user_packs_dir: Option<String>,
    pub permission: Permission,
    pub audio: AudioStatus,
    /// The app in front now, TakTak's own windows excluded (M4). `None` when unknown, without a
    /// bundle id, or where rules are unsupported. Current value only: never logged or persisted.
    pub frontmost_app: Option<AppRef>,
    /// `settings.app_rule` silences `frontmost_app` right now (M4).
    pub rule_blocked: bool,
    /// Why TakTak muted itself (M4); `None` = not auto-muted.
    pub auto_mute: Option<AutoMute>,
    /// Per-app rules work on this platform (M4): macOS, once the frontmost-app watcher runs.
    pub rules_supported: bool,
    /// (M4)
    pub onboarding: OnboardingStatus,
    /// Each auto-mute reason on its own (M4), from which `auto_mute` is derived. Internal: not
    /// part of the contract, never serialized.
    #[serde(skip)]
    pub auto_mute_reasons: AutoMuteReasons,
}

impl AppState {
    /// The state before anything has been scanned, loaded or started.
    pub fn initial(version: impl Into<String>, settings: Settings) -> AppState {
        let onboarding =
            OnboardingStatus { offer: !settings.onboarding_done, ..OnboardingStatus::default() };
        AppState {
            version: version.into(),
            settings,
            muted: false,
            playing: false,
            packs: Vec::new(),
            invalid_packs: Vec::new(),
            playing_pack_id: None,
            active_pack_error: None,
            mute_hotkey_error: None,
            user_packs_dir: None,
            permission: Permission::Unknown,
            audio: AudioStatus::default(),
            frontmost_app: None,
            rule_blocked: false,
            auto_mute: None,
            // The service turns these on where the platform supports them.
            rules_supported: false,
            onboarding,
            auto_mute_reasons: AutoMuteReasons::default(),
        }
    }
}

/// Keystroke-to-sound timings since the settings window opened. Timings only, never keys.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatencyReport {
    pub count: usize,
    pub total_p50_ms: f64,
    pub total_p95_ms: f64,
    pub total_max_ms: f64,
    pub input_p50_ms: f64,
    pub queue_p50_ms: f64,
    pub output_ms: f64,
}

/// Which picker `import_mechvibes_pack` opens (M5). macOS shows one panel that takes a folder
/// or a `.zip` whatever the kind; the Windows and Linux pickers choose one or the other, `Any`
/// being the `.zip` picker there.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PickKind {
    #[default]
    Any,
    Folder,
    Zip,
}

/// What an import wrote (M5). Every string is pack content, already escaped for display.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    /// The new pack's id (`mv-<name>`), as it will appear in `AppState::packs`.
    pub id: String,
    pub name: String,
    /// The source folder or `.zip` file name.
    pub source: String,
    /// The Mechvibes flavour, e.g. "Mechvibes v2".
    pub format: String,
    /// Keys with a sound of their own.
    pub keys_mapped: usize,
    /// How many of them also have a release sound of their own.
    pub keys_with_release: usize,
    /// Sound files written (the preview not counted).
    pub sounds_written: usize,
    /// An earlier import of the same pack was replaced.
    pub replaced: bool,
    /// Skipped keys, missing or unreadable files and other notes, at most
    /// [`crate::mechvibes::MAX_WARNINGS`] lines.
    pub warnings: Vec<String>,
}

/// The result of `import_mechvibes_pack` and `overwrite_mechvibes_pack` (M5).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum MechvibesImport {
    /// Written to the user packs folder; the registry lists it within about a second.
    Imported { pack: ImportSummary },
    /// This pack (same id, same source name) was imported before; nothing was changed.
    /// `overwrite_mechvibes_pack` replaces it.
    AlreadyImported { id: String, source: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_with_contract_names() {
        let json = serde_json::to_value(Settings::default()).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "enabled": true,
                "packId": "buckling-spring",
                "masterVolume": 0.7,
                "pressVolume": 1.0,
                "releaseVolume": 1.0,
                "variantMode": "consistent",
                "humanize": 0.25,
                "muteHotkey": "CommandOrControl+Alt+Shift+M",
                "launchAtLogin": false,
                "appRule": { "mode": "everywhere", "apps": [] },
                "muteOnOutputChange": false,
                "onboardingDone": false,
            })
        );
        let back: Settings = serde_json::from_value(json).unwrap();
        assert_eq!(back, Settings::default());
    }

    #[test]
    fn app_rules_round_trip_with_contract_names() {
        let settings = Settings {
            app_rule: AppRule {
                mode: AppRuleMode::Never,
                apps: vec![
                    AppRuleEntry { id: "com.tinyspeck.slackmacgap".into(), name: "Slack".into() },
                    AppRuleEntry { id: "us.zoom.xos".into(), name: "zoom.us".into() },
                ],
            },
            mute_on_output_change: true,
            onboarding_done: true,
            ..Settings::default()
        };
        let json = serde_json::to_value(&settings).unwrap();
        assert_eq!(
            json["appRule"],
            serde_json::json!({
                "mode": "never",
                "apps": [
                    { "id": "com.tinyspeck.slackmacgap", "name": "Slack" },
                    { "id": "us.zoom.xos", "name": "zoom.us" },
                ],
            })
        );
        assert_eq!(json["muteOnOutputChange"], true);
        assert_eq!(json["onboardingDone"], true);
        let back: Settings = serde_json::from_value(json).unwrap();
        assert_eq!(back, settings);
        for (mode, name) in [
            (AppRuleMode::Everywhere, "everywhere"),
            (AppRuleMode::Only, "only"),
            (AppRuleMode::Never, "never"),
        ] {
            assert_eq!(serde_json::to_value(mode).unwrap(), name);
            assert_eq!(serde_json::from_value::<AppRuleMode>(name.into()).unwrap(), mode);
        }
    }

    /// A Milestone 3 file has none of the Milestone 4 fields; serde fills in the defaults.
    #[test]
    fn milestone_3_settings_take_the_new_defaults() {
        let s: Settings = serde_json::from_str(
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
        assert_eq!(s.pack_id, "typewriter");
        assert_eq!(s.app_rule, AppRule { mode: AppRuleMode::Everywhere, apps: vec![] });
        assert!(!s.mute_on_output_change);
        assert!(!s.onboarding_done, "serde default; the loader migrates existing files");
    }

    #[test]
    fn rule_entries_ignore_unknown_fields_and_default_the_name() {
        // A newer version may add per-app overrides; this one ignores them.
        let rule: AppRule = serde_json::from_str(
            r#"{"apps":[{"id":"com.apple.Safari","packId":"typewriter","volume":0.5}]}"#,
        )
        .unwrap();
        assert_eq!(rule.mode, AppRuleMode::Everywhere);
        assert_eq!(
            rule.apps,
            [AppRuleEntry { id: "com.apple.Safari".into(), name: String::new() }]
        );
        // An entry needs an id.
        assert!(serde_json::from_str::<AppRuleEntry>(r#"{"name":"Safari"}"#).is_err());
    }

    #[test]
    fn missing_settings_fields_take_defaults() {
        let s: Settings =
            serde_json::from_str(r#"{"packId":"typewriter","muteHotkey":null}"#).unwrap();
        assert_eq!(s.pack_id, "typewriter");
        assert_eq!(s.mute_hotkey, None);
        assert_eq!(s.master_volume, 0.7);
    }

    #[test]
    fn app_state_uses_contract_names() {
        let mut state = AppState::initial("0.1.0", Settings::default());
        state.packs.push(PackSummary {
            id: "x".into(),
            name: "X".into(),
            author: "A".into(),
            license: "MIT".into(),
            description: None,
            attribution: None,
            origin: CorePackOrigin::User.into(),
            has_release: true,
            per_key: false,
            warnings: vec![],
        });
        let json = serde_json::to_value(&state).unwrap();
        let mut keys: Vec<&str> = json.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "activePackError",
                "audio",
                "autoMute",
                "frontmostApp",
                "invalidPacks",
                "muteHotkeyError",
                "muted",
                "onboarding",
                "packs",
                "permission",
                "playing",
                "playingPackId",
                "ruleBlocked",
                "rulesSupported",
                "settings",
                "userPacksDir",
                "version",
            ]
        );
        assert_eq!(json["frontmostApp"], serde_json::Value::Null);
        assert_eq!(json["autoMute"], serde_json::Value::Null);
        assert_eq!(json["ruleBlocked"], false);
        assert_eq!(json["rulesSupported"], false);
        assert_eq!(
            json["onboarding"],
            serde_json::json!({
                "offer": true,
                "permissionRequired": false,
                "relaunchSuggested": false,
                "inputGroupNeeded": false,
            })
        );
        assert_eq!(json["permission"], "unknown");
        assert_eq!(json["playingPackId"], serde_json::Value::Null);
        assert_eq!(json["muteHotkeyError"], serde_json::Value::Null);
        assert_eq!(json["audio"]["state"], "starting");
        assert_eq!(json["audio"]["sampleRate"], serde_json::Value::Null);
        assert_eq!(json["packs"][0]["origin"], "user");
        assert_eq!(json["packs"][0]["hasRelease"], true);
        assert_eq!(json["packs"][0]["perKey"], false);
    }

    #[test]
    fn milestone_4_state_uses_contract_names() {
        let mut state =
            AppState::initial("0.1.0", Settings { onboarding_done: true, ..Settings::default() });
        assert!(!state.onboarding.offer, "done, and no permission known to be missing");
        state.frontmost_app = Some(AppRef { id: "com.apple.Safari".into(), name: "Safari".into() });
        state.rule_blocked = true;
        state.rules_supported = true;
        state.onboarding = OnboardingStatus {
            offer: true,
            permission_required: true,
            relaunch_suggested: true,
            input_group_needed: true,
        };
        for (auto_mute, name) in
            [(AutoMute::ScreenLocked, "screenLocked"), (AutoMute::OutputChanged, "outputChanged")]
        {
            state.auto_mute = Some(auto_mute);
            let json = serde_json::to_value(&state).unwrap();
            assert_eq!(json["autoMute"], name);
            assert_eq!(
                json["frontmostApp"],
                serde_json::json!({ "id": "com.apple.Safari", "name": "Safari" })
            );
            assert_eq!(json["ruleBlocked"], true);
            assert_eq!(json["rulesSupported"], true);
            assert_eq!(
                json["onboarding"],
                serde_json::json!({
                    "offer": true,
                    "permissionRequired": true,
                    "relaunchSuggested": true,
                    "inputGroupNeeded": true,
                })
            );
        }
        let info = AppInfo {
            id: "com.apple.Safari".into(),
            name: "Safari".into(),
            icon_data_url: Some("data:image/png;base64,AAAA".into()),
        };
        assert_eq!(
            serde_json::to_value(&info).unwrap(),
            serde_json::json!({
                "id": "com.apple.Safari",
                "name": "Safari",
                "iconDataUrl": "data:image/png;base64,AAAA",
            })
        );
        let no_icon = AppInfo { icon_data_url: None, ..info };
        assert_eq!(serde_json::to_value(&no_icon).unwrap()["iconDataUrl"], serde_json::Value::Null);
    }

    #[test]
    fn mechvibes_imports_use_contract_names() {
        let pack = ImportSummary {
            id: "mv-cream".into(),
            name: "Cream".into(),
            source: "cream.zip".into(),
            format: "Mechvibes v2".into(),
            keys_mapped: 80,
            keys_with_release: 12,
            sounds_written: 92,
            replaced: false,
            warnings: vec!["Skipped key \"999\": unknown key code".into()],
        };
        assert_eq!(
            serde_json::to_value(MechvibesImport::Imported { pack }).unwrap(),
            serde_json::json!({
                "outcome": "imported",
                "pack": {
                    "id": "mv-cream",
                    "name": "Cream",
                    "source": "cream.zip",
                    "format": "Mechvibes v2",
                    "keysMapped": 80,
                    "keysWithRelease": 12,
                    "soundsWritten": 92,
                    "replaced": false,
                    "warnings": ["Skipped key \"999\": unknown key code"],
                },
            })
        );
        assert_eq!(
            serde_json::to_value(MechvibesImport::AlreadyImported {
                id: "mv-cream".into(),
                source: "cream.zip".into(),
            })
            .unwrap(),
            serde_json::json!({ "outcome": "alreadyImported", "id": "mv-cream", "source": "cream.zip" })
        );
        for (kind, name) in
            [(PickKind::Any, "any"), (PickKind::Folder, "folder"), (PickKind::Zip, "zip")]
        {
            assert_eq!(serde_json::from_value::<PickKind>(name.into()).unwrap(), kind);
        }
    }

    #[test]
    fn latency_report_uses_contract_names() {
        let r = LatencyReport {
            count: 5,
            total_p50_ms: 1.0,
            total_p95_ms: 2.0,
            total_max_ms: 3.0,
            input_p50_ms: 0.5,
            queue_p50_ms: 0.25,
            output_ms: 4.0,
        };
        let json = serde_json::to_value(&r).unwrap();
        for k in [
            "count",
            "totalP50Ms",
            "totalP95Ms",
            "totalMaxMs",
            "inputP50Ms",
            "queueP50Ms",
            "outputMs",
        ] {
            assert!(json.get(k).is_some(), "{k}");
        }
    }
}
