//! The state the UI renders, mirroring `docs/ui-contract.md` field for field.
//!
//! Every type serializes with camelCase field names and lowercase enum values, exactly as the
//! TypeScript types in `src/lib/types.ts`. Nothing here ever holds a key identity or typed text.

use serde::{Deserialize, Serialize};
use taktak_core::audio::{DEFAULT_HUMANIZE, VariantMode as CoreVariantMode};
use taktak_core::pack::PackOrigin as CorePackOrigin;

/// The pack selected when nothing else is (fresh install, saved pack gone).
pub const DEFAULT_PACK_ID: &str = "deep-thock";
/// The mute hotkey on a fresh install.
pub const DEFAULT_MUTE_HOTKEY: &str = "CommandOrControl+Alt+Shift+M";

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
        }
    }
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
    /// Hotkey/tray mute, separate from `settings.enabled`.
    pub muted: bool,
    /// `enabled && !muted && permission granted && audio ok`.
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
}

impl AppState {
    /// The state before anything has been scanned, loaded or started.
    pub fn initial(version: impl Into<String>, settings: Settings) -> AppState {
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
                "packId": "deep-thock",
                "masterVolume": 0.7,
                "pressVolume": 1.0,
                "releaseVolume": 1.0,
                "variantMode": "consistent",
                "humanize": 0.25,
                "muteHotkey": "CommandOrControl+Alt+Shift+M",
                "launchAtLogin": false,
            })
        );
        let back: Settings = serde_json::from_value(json).unwrap();
        assert_eq!(back, Settings::default());
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
                "invalidPacks",
                "muteHotkeyError",
                "muted",
                "packs",
                "permission",
                "playing",
                "playingPackId",
                "settings",
                "userPacksDir",
                "version",
            ]
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
