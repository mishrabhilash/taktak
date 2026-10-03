//! The commands the UI invokes, one per row of `docs/ui-contract.md` § Commands. Thin glue over
//! [`Service`]: each change goes through `Service::update`, which broadcasts `state-changed`.
//!
//! Synchronous commands run on the main thread, so none of them waits on I/O or decoding:
//! packs load on `taktak-loader`, previews and engine work happen on `taktak-control`.
//! Commands that start other programs are `async` (they run on the async runtime's threads).
//!
//! Every command rejects with a user-facing string on error.

use crate::service::Service;
use crate::settings::unit;
use crate::state::{AppState, LatencyReport, VariantMode};
use crate::{hotkey, system, windows};
use std::path::PathBuf;
use std::time::Duration;
use tauri::{AppHandle, Runtime, State};
use tauri_plugin_autostart::ManagerExt;

/// Sent with the whole [`AppState`] after every change.
pub const STATE_CHANGED: &str = "state-changed";

/// How long `preview_pack` waits for its clip to start. Decoding a large user pack can take
/// longer; the command then resolves and the clip plays when it is ready.
const PREVIEW_WAIT: Duration = Duration::from_secs(5);

/// What the commands return: the value, or a message to show the user.
pub type CmdResult<T> = Result<T, String>;

#[tauri::command]
pub fn get_state<R: Runtime>(window: tauri::Window<R>, service: State<'_, Service>) -> AppState {
    log::debug!("state requested by the {} window", window.label());
    service.snapshot()
}

#[tauri::command]
pub fn set_enabled(service: State<'_, Service>, enabled: bool) -> AppState {
    service.update(|s| s.settings.enabled = enabled)
}

#[tauri::command]
pub fn set_muted(service: State<'_, Service>, muted: bool) -> AppState {
    service.update(|s| s.muted = muted)
}

#[tauri::command]
pub fn set_pack(service: State<'_, Service>, id: String) -> CmdResult<AppState> {
    service.set_pack(&id)
}

#[tauri::command]
pub fn set_master_volume(service: State<'_, Service>, value: f64) -> AppState {
    service.update(|s| s.settings.master_volume = unit(value))
}

#[tauri::command]
pub fn set_press_volume(service: State<'_, Service>, value: f64) -> AppState {
    service.update(|s| s.settings.press_volume = unit(value))
}

#[tauri::command]
pub fn set_release_volume(service: State<'_, Service>, value: f64) -> AppState {
    service.update(|s| s.settings.release_volume = unit(value))
}

#[tauri::command]
pub fn set_variant_mode(service: State<'_, Service>, mode: VariantMode) -> AppState {
    service.update(|s| s.settings.variant_mode = mode)
}

#[tauri::command]
pub fn set_humanize(service: State<'_, Service>, value: f64) -> AppState {
    service.update(|s| s.settings.humanize = unit(value))
}

/// Registers the new hotkey before saving it; an empty string means no hotkey. Success also
/// clears `muteHotkeyError` (the saved hotkey that failed at startup is replaced).
#[tauri::command]
pub fn set_mute_hotkey<R: Runtime>(
    app: AppHandle<R>,
    service: State<'_, Service>,
    accelerator: Option<String>,
) -> CmdResult<AppState> {
    let accelerator = accelerator.map(|a| a.trim().to_owned()).filter(|a| !a.is_empty());
    hotkey::apply(&app, accelerator.as_deref())?;
    Ok(service.update(|s| {
        s.settings.mute_hotkey = accelerator;
        s.mute_hotkey_error = None;
    }))
}

#[tauri::command]
pub fn set_launch_at_login<R: Runtime>(
    app: AppHandle<R>,
    service: State<'_, Service>,
    enabled: bool,
) -> CmdResult<AppState> {
    let autostart = app.autolaunch();
    let result = if enabled { autostart.enable() } else { autostart.disable() };
    result.map_err(|e| {
        log::warn!("launch at login: {e}");
        let action = if enabled { "turn on" } else { "turn off" };
        format!("Could not {action} launch at login: {e}")
    })?;
    Ok(service.update(|s| s.settings.launch_at_login = enabled))
}

/// Resolves once the clip plays; rejects when it cannot (no output device, or the pack's
/// sounds cannot be decoded). Async: it waits for the control thread, never on the main thread.
#[tauri::command]
pub async fn preview_pack(service: State<'_, Service>, id: String) -> CmdResult<()> {
    let wait = service.preview(&id)?;
    tauri::async_runtime::spawn_blocking(move || wait.wait(PREVIEW_WAIT))
        .await
        .map_err(|e| format!("The preview could not be started: {e}"))?
}

#[tauri::command]
pub fn stop_preview(service: State<'_, Service>) {
    service.stop_preview();
}

/// Async so the window is built off the main thread (WebView2 deadlocks otherwise).
#[tauri::command]
pub async fn open_settings<R: Runtime>(app: AppHandle<R>) -> CmdResult<()> {
    windows::show_settings(&app).map_err(|e| format!("Could not open the settings window: {e}"))
}

/// Hides the tray popover (Escape) and, on macOS, hands the keyboard back to the app the user
/// was in.
#[tauri::command]
pub fn hide_tray<R: Runtime>(app: AppHandle<R>) -> CmdResult<()> {
    windows::hide_tray(&app).map_err(|e| format!("Could not hide the popover: {e}"))
}

#[tauri::command]
pub async fn open_user_packs_dir(service: State<'_, Service>) -> CmdResult<()> {
    let dir = service.snapshot().user_packs_dir.map(PathBuf::from);
    let dir = dir.ok_or("This system has no folder for your own packs.")?;
    system::reveal_dir(&dir).map_err(|e| {
        log::warn!("cannot reveal {}: {e}", dir.display());
        format!("Could not open {}: {e}", dir.display())
    })
}

#[tauri::command]
pub async fn open_permission_settings() -> CmdResult<()> {
    system::open_input_monitoring().map_err(|e| {
        log::warn!("cannot open the Input Monitoring settings: {e}");
        "Could not open System Settings. Open Privacy & Security → Input Monitoring there."
            .to_owned()
    })
}

#[tauri::command]
pub fn get_latency(service: State<'_, Service>) -> Option<LatencyReport> {
    service.latency()
}

/// Exits; the run loop's `Exit` handler saves settings and stops the audio first.
#[tauri::command]
pub fn quit<R: Runtime>(app: AppHandle<R>) {
    app.exit(0);
}
