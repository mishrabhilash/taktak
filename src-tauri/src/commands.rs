//! The commands the UI invokes, one per row of `docs/ui-contract.md` § Commands. Thin glue over
//! [`Service`]: each change goes through `Service::update`, which broadcasts `state-changed`.
//!
//! Synchronous commands run on the main thread, so none of them waits on I/O or decoding:
//! packs load on `taktak-loader`, previews and engine work happen on `taktak-control`.
//! Commands that start other programs, create windows or need AppKit (running apps, icons, the
//! app picker: done on the main thread, waited for here) are `async` (they run on the async
//! runtime's threads).
//!
//! Every command rejects with a user-facing string on error.

use crate::service::Service;
use crate::settings::unit;
use crate::state::{
    AppInfo, AppRuleMode, AppState, LatencyReport, MechvibesImport, PickKind, VariantMode,
};
use crate::{
    apps, automute, hotkey, mechvibes, permission, relaunch as restart, rules, system, tray,
    windows,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;
use tauri::{AppHandle, Runtime, State};
use tauri_plugin_autostart::ManagerExt;

/// Sent with the whole [`AppState`] after every change.
pub const STATE_CHANGED: &str = "state-changed";

/// How long `preview_pack` waits for its clip to start. Decoding a large user pack can take
/// longer; the command then resolves and the clip plays when it is ready.
const PREVIEW_WAIT: Duration = Duration::from_secs(5);

/// Icons rendered per trip to the main thread, so a long list never holds it for long.
const ICON_BATCH: usize = 12;

/// What the commands return: the value, or a message to show the user.
pub type CmdResult<T> = Result<T, String>;

/// Runs `task` on the main thread (AppKit wants it there) and waits for its result here, off
/// the main thread.
async fn on_main<R: Runtime, T: Send + 'static>(
    app: &AppHandle<R>,
    task: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    let (tx, rx) = mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(task());
    })
    .map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || rx.recv())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_state<R: Runtime>(window: tauri::Window<R>, service: State<'_, Service>) -> AppState {
    log::debug!("state requested by the {} window", window.label());
    service.snapshot()
}

/// Turning sounds on also clears an `outputChanged` auto-mute.
#[tauri::command]
pub fn set_enabled(service: State<'_, Service>, enabled: bool) -> AppState {
    service.update(|s| automute::set_enabled(s, enabled))
}

/// Either value also clears an `outputChanged` auto-mute (the manual mute takes over); a
/// `screenLocked` one stays.
#[tauri::command]
pub fn set_muted(service: State<'_, Service>, muted: bool) -> AppState {
    service.update(|s| automute::set_muted(s, muted))
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

/// macOS: asks macOS to list TakTak under Input Monitoring if it never decided (on the main
/// thread, waiting for the entry to appear: see [`permission`]), then opens that pane unless the
/// request granted access. Resolves with whether TakTak is in the list (true elsewhere).
#[tauri::command]
pub async fn open_permission_settings<R: Runtime>(app: AppHandle<R>) -> CmdResult<bool> {
    let handle = app.clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || permission::ask(&handle))
        .await
        .map_err(|e| format!("Could not ask macOS for Input Monitoring: {e}"))?;
    if outcome.open_pane {
        system::open_input_monitoring().map_err(|e| {
            log::warn!("cannot open the Input Monitoring settings: {e}");
            "Could not open System Settings (System Preferences on macOS 12 and earlier). Open \
             Privacy & Security → Input Monitoring there."
                .to_owned()
        })?;
    }
    Ok(outcome.listed)
}

/// Shows TakTak itself (macOS: `TakTak.app`) in the file manager, to drag it into the Input
/// Monitoring list or find it from its + button.
#[tauri::command]
pub async fn reveal_app() -> CmdResult<()> {
    let exe = std::env::current_exe().map_err(|e| format!("Could not find TakTak: {e}"))?;
    let target = restart::bundle_of(&exe).unwrap_or(exe);
    system::reveal_file(&target).map_err(|e| {
        log::warn!("cannot reveal {}: {e}", target.display());
        format!("Could not show {}: {e}", target.display())
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

/// The running regular apps (`[]` where per-app rules are unsupported), with their icons.
#[tauri::command]
pub async fn list_running_apps<R: Runtime>(
    app: AppHandle<R>,
    service: State<'_, Service>,
) -> CmdResult<Vec<AppInfo>> {
    if !service.snapshot().rules_supported {
        return Ok(Vec::new());
    }
    let running = on_main(&app, apps::running_apps).await.map_err(|e| {
        log::warn!("cannot list the running apps: {e}");
        "Could not list the running apps.".to_owned()
    })?;
    let ids: Vec<String> = running.iter().map(|a| a.id.clone()).collect();
    let mut icons = app_icons(&app, ids, true).await?;
    Ok(running
        .into_iter()
        .map(|a| AppInfo { icon_data_url: icons.remove(&a.id).flatten(), id: a.id, name: a.name })
        .collect())
}

/// Icons for `ids`, rendered on the main thread a few at a time; cached if `keep`
/// ([`apps::icons`]).
async fn app_icons<R: Runtime>(
    app: &AppHandle<R>,
    ids: Vec<String>,
    keep: bool,
) -> CmdResult<HashMap<String, Option<String>>> {
    let mut icons = HashMap::with_capacity(ids.len());
    for batch in ids.chunks(ICON_BATCH) {
        let batch = batch.to_vec();
        let found = on_main(app, move || apps::icons(&batch, keep)).await.map_err(|e| {
            log::warn!("cannot look up app icons: {e}");
            "Could not load the app icons.".to_owned()
        })?;
        icons.extend(found);
    }
    Ok(icons)
}

/// The native picker for an app bundle; resolves when it closes (`null`: cancelled). It does
/// not add the app.
#[tauri::command]
pub async fn choose_app<R: Runtime>(
    app: AppHandle<R>,
    service: State<'_, Service>,
) -> CmdResult<Option<AppInfo>> {
    if !service.snapshot().rules_supported {
        return Err(apps::UNSUPPORTED.to_owned());
    }
    let (tx, rx) = mpsc::channel();
    app.run_on_main_thread(move || {
        apps::choose_app(Box::new(move |result| {
            let _ = tx.send(result);
        }));
    })
    .map_err(|e| format!("The app chooser could not be opened: {e}"))?;
    tauri::async_runtime::spawn_blocking(move || rx.recv())
        .await
        .map_err(|e| format!("The app chooser could not be opened: {e}"))?
        .map_err(|_| "The app chooser closed unexpectedly.".to_owned())?
}

/// An icon for each id (at most 200), for listed apps that are not running. Only the icons of
/// apps on the rule list are cached; any other id (the frontmost app, for one) is looked up
/// once and not kept, so the cache never collects the apps the user brought to the front.
#[tauri::command]
pub async fn get_app_icons<R: Runtime>(
    app: AppHandle<R>,
    service: State<'_, Service>,
    ids: Vec<String>,
) -> CmdResult<HashMap<String, Option<String>>> {
    let ids = apps::icon_ids(ids);
    if !service.snapshot().rules_supported {
        return Ok(ids.into_iter().map(|id| (id, None)).collect());
    }
    let settings = service.snapshot().settings;
    let (keep, once) =
        apps::icon_lookups(ids, settings.app_rule.apps.iter().map(|a| a.id.as_str()));
    let mut icons = app_icons(&app, keep, true).await?;
    icons.extend(app_icons(&app, once, false).await?);
    Ok(icons)
}

#[tauri::command]
pub fn set_app_rule_mode(service: State<'_, Service>, mode: AppRuleMode) -> AppState {
    service.update(|s| s.settings.app_rule.mode = mode)
}

/// Appends the app to the rule list (an id already listed changes nothing).
#[tauri::command]
pub fn add_rule_app(service: State<'_, Service>, id: String, name: String) -> CmdResult<AppState> {
    service.try_update(|s| {
        rules::add(&mut s.settings.app_rule, &id, &name).map(drop).map_err(str::to_owned)
    })
}

#[tauri::command]
pub fn remove_rule_app(service: State<'_, Service>, id: String) -> AppState {
    service.update(|s| {
        rules::remove(&mut s.settings.app_rule, &id);
    })
}

/// Turning it off also clears an `outputChanged` auto-mute.
#[tauri::command]
pub fn set_mute_on_output_change(service: State<'_, Service>, enabled: bool) -> AppState {
    service.update(|s| automute::set_mute_on_output_change(s, enabled))
}

/// Async so the window is built off the main thread (WebView2 deadlocks otherwise).
#[tauri::command]
pub async fn open_onboarding<R: Runtime>(app: AppHandle<R>) -> CmdResult<()> {
    windows::show_onboarding(&app).map_err(|e| format!("Could not open the welcome guide: {e}"))
}

/// Marks the onboarding as done, then closes its window (off this IPC call: the window may be
/// the one asking).
#[tauri::command]
pub fn finish_onboarding<R: Runtime>(app: AppHandle<R>, service: State<'_, Service>) -> AppState {
    let state = service.update(|s| s.settings.onboarding_done = true);
    tray::spawn_window_task(&app, windows::close_onboarding);
    state
}

/// Quits like `quit` and starts TakTak again (reopening the onboarding window if it is open).
/// Rejects, without quitting, when the new instance cannot be started.
#[tauri::command]
pub async fn relaunch<R: Runtime>(app: AppHandle<R>) -> CmdResult<()> {
    let onboarding = windows::onboarding_open(&app);
    restart::spawn(onboarding).map_err(|e| {
        log::warn!("cannot start TakTak again: {e}");
        "TakTak could not restart itself. Quit it and open it again.".to_owned()
    })?;
    log::info!("restarting");
    windows::mark_exiting();
    app.exit(0);
    Ok(())
}

/// The user packs folder, or a message when this system has none.
fn user_packs_dir(service: &Service) -> CmdResult<PathBuf> {
    service
        .snapshot()
        .user_packs_dir
        .map(PathBuf::from)
        .ok_or_else(|| mechvibes::NO_USER_DIR.into())
}

/// Opens the native picker for a pack folder or `.zip` and resolves when it closes: the chosen
/// path, or `None` when cancelled. Never on the main thread (macOS: the panel runs there, this
/// waits for it here).
async fn pick_pack<R: Runtime>(app: &AppHandle<R>, kind: PickKind) -> CmdResult<Option<PathBuf>> {
    const NOT_OPENED: &str = "The pack chooser could not be opened.";
    #[cfg(target_os = "macos")]
    let picked = {
        let _ = kind; // one panel takes both
        let (tx, rx) = mpsc::channel();
        app.run_on_main_thread(move || {
            mechvibes::pick(Box::new(move |result| {
                let _ = tx.send(result);
            }));
        })
        .map_err(|e| format!("{NOT_OPENED} {e}"))?;
        tauri::async_runtime::spawn_blocking(move || rx.recv())
            .await
            .map_err(|e| format!("{NOT_OPENED} {e}"))?
            .map_err(|_| "The pack chooser closed unexpectedly.".to_owned())?
    };
    #[cfg(not(target_os = "macos"))]
    let picked = {
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || mechvibes::pick_blocking(&app, kind))
            .await
            .map_err(|e| format!("{NOT_OPENED} {e}"))?
    };
    picked
}

/// Imports on a worker thread (decoding and writing can take seconds for a large pack).
async fn run_import(src: PathBuf, dest: PathBuf, overwrite: bool) -> CmdResult<MechvibesImport> {
    tauri::async_runtime::spawn_blocking(move || mechvibes::import(&src, &dest, overwrite))
        .await
        .map_err(|e| format!("The import stopped unexpectedly: {e}"))?
}

/// (M5) "Import Mechvibes pack…": the native picker (a folder or a `.zip`), then the import into
/// the user packs folder on a worker thread; the registry's hot reload lists the new pack within
/// about a second. `None` when the picker was cancelled. One import at a time.
#[tauri::command]
pub async fn import_mechvibes_pack<R: Runtime>(
    app: AppHandle<R>,
    service: State<'_, Service>,
    kind: PickKind,
) -> CmdResult<Option<MechvibesImport>> {
    let dest = user_packs_dir(&service)?;
    let _busy = mechvibes::Busy::take()?;
    let Some(src) = pick_pack(&app, kind).await? else {
        return Ok(None);
    };
    run_import(src, dest, false).await.map(Some)
}

/// (M5) Replaces the earlier import that the last `import_mechvibes_pack` found (its
/// `alreadyImported` outcome), from the same source. Rejects when there is none.
#[tauri::command]
pub async fn overwrite_mechvibes_pack(service: State<'_, Service>) -> CmdResult<MechvibesImport> {
    let dest = user_packs_dir(&service)?;
    let _busy = mechvibes::Busy::take()?;
    let src = mechvibes::take_pending().ok_or(mechvibes::NOTHING_TO_OVERWRITE)?;
    run_import(src, dest, true).await
}
