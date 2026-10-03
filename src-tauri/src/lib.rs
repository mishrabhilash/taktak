//! TakTak's Tauri app: the tray icon, the tray popover and settings windows, and the commands
//! the UI calls (`docs/ui-contract.md`). Everything latency-sensitive lives in `taktak-core`;
//! this crate runs on the main thread and ordinary worker threads, never on the audio
//! callback or the input hook.
//!
//! - [`service`]: the state and the control thread that owns the engine, listener and packs
//!   (logic in [`catalog`], [`input`], [`loader`], [`settings`]; all unit-tested).
//! - [`commands`], [`tray`], [`windows`], [`hotkey`]: thin Tauri glue.
//! - `instance` (macOS): one TakTak per user; a second launch hands over and exits.
//! - [`selftest`]: `taktak --selftest`, a headless check for CI.

pub mod catalog;
pub mod commands;
pub mod hotkey;
pub mod input;
#[cfg(target_os = "macos")]
pub mod instance;
pub mod loader;
pub mod logging;
pub mod selftest;
pub mod service;
pub mod settings;
pub mod state;
pub mod system;
pub mod tray;
pub mod windows;

use commands::STATE_CHANGED;
use service::{Config, Notify, Service};
use state::AppState;
use std::sync::mpsc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, RunEvent, Runtime};
use tauri_plugin_autostart::ManagerExt;

/// How long quitting waits for settings to be written and the audio to stop.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

/// Builds and runs the app until the user quits, or runs the self-test (`--selftest`).
pub fn run() {
    logging::init();
    let context = tauri::generate_context!();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--selftest") {
        // Before the single-instance check, which would hand the run to an instance already open.
        let version = context.package_info().version.to_string();
        let resources =
            tauri::utils::platform::resource_dir(context.package_info(), &tauri::Env::default())
                .ok();
        std::process::exit(selftest::run(&version, resources, &args));
    }

    // Before anything starts: a second launch only hands over to the running instance.
    #[cfg(target_os = "macos")]
    let instance = match instance::default_dir().map(|dir| instance::claim(&dir)) {
        Some(Ok(instance::Claim::First(instance))) => Some(instance),
        Some(Ok(instance::Claim::Second)) => {
            log::info!("TakTak is already running; asked it to show its settings window");
            std::process::exit(0);
        }
        Some(Err(e)) => {
            log::warn!("cannot make sure only one TakTak runs: {e}");
            None
        }
        None => None,
    };

    let builder = tauri::Builder::default();
    // First, so a second launch hands over to the running instance before anything starts.
    #[cfg(not(target_os = "macos"))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
        tray::spawn_window_task(app, windows::show_settings);
    }));
    let app = builder
        .plugin(tauri_plugin_global_shortcut::Builder::new().with_handler(hotkey::on_event).build())
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .plugin(tauri_plugin_positioner::init())
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            {
                // Menu-bar app: no Dock icon, no app menu.
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                if let Some(instance) = instance {
                    let handle = app.handle().clone();
                    let listening = instance.listen(move || {
                        log::info!("launched again; showing the settings window");
                        tray::spawn_window_task(&handle, windows::show_settings);
                    });
                    if let Err(e) = listening {
                        // Still the only instance; a second launch just cannot open Settings.
                        log::warn!("second launches cannot reach TakTak: {e}");
                    }
                    app.manage(instance);
                }
            }
            setup(app.handle())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::set_enabled,
            commands::set_muted,
            commands::set_pack,
            commands::set_master_volume,
            commands::set_press_volume,
            commands::set_release_volume,
            commands::set_variant_mode,
            commands::set_humanize,
            commands::set_mute_hotkey,
            commands::set_launch_at_login,
            commands::preview_pack,
            commands::stop_preview,
            commands::open_settings,
            commands::hide_tray,
            commands::open_user_packs_dir,
            commands::open_permission_settings,
            commands::get_latency,
            commands::quit,
        ])
        .build(context);

    let app = match app {
        Ok(app) => app,
        Err(e) => {
            log::error!("TakTak could not start: {e}");
            std::process::exit(1);
        }
    };
    #[cfg(unix)]
    quit_on_signals(app.handle());
    app.run(|app, event| match event {
        // Closing the last window must not quit a tray app; only `quit` (code Some) does.
        RunEvent::ExitRequested { code: None, api, .. } => api.prevent_exit(),
        RunEvent::Exit => {
            if let Some(service) = app.try_state::<Service>() {
                service.shutdown(SHUTDOWN_TIMEOUT);
            }
            #[cfg(target_os = "macos")]
            if let Some(instance) = app.try_state::<instance::Instance>() {
                instance.release();
            }
        }
        // Opening TakTak again from Finder, Spotlight, Launchpad or `open` while it runs starts
        // no second process: macOS sends this instead. With no Dock icon, it is the way back in
        // when the menu-bar icon is hidden (behind the notch, say).
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => {
            log::info!("opened again; showing the settings window");
            tray::spawn_window_task(app, windows::show_settings);
        }
        _ => {}
    });
}

/// Quits like the `quit` command on SIGTERM or SIGINT (`kill`, Ctrl+C under `tauri dev`), so
/// settings are saved and the audio stops first. A second signal exits at once.
#[cfg(unix)]
fn quit_on_signals<R: Runtime>(app: &AppHandle<R>) {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::signal::unix::{SignalKind, signal};

    let asked = Arc::new(AtomicBool::new(false));
    for kind in [SignalKind::terminate(), SignalKind::interrupt()] {
        let (app, asked) = (app.clone(), Arc::clone(&asked));
        tauri::async_runtime::spawn(async move {
            let mut signals = match signal(kind) {
                Ok(signals) => signals,
                Err(e) => {
                    log::warn!("cannot handle signal {}: {e}", kind.as_raw_value());
                    return;
                }
            };
            while signals.recv().await.is_some() {
                if asked.swap(true, Ordering::Relaxed) {
                    std::process::exit(1);
                }
                log::info!("quitting on signal {}", kind.as_raw_value());
                app.exit(0);
            }
        });
    }
}

/// Loads the settings, starts the service, registers the hotkey and creates the tray.
fn setup<R: Runtime>(app: &AppHandle<R>) -> Result<(), Box<dyn std::error::Error>> {
    let settings_path = app.path().app_config_dir()?.join(settings::FILE_NAME);
    let loaded = settings::load(&settings_path);
    if let Some(note) = &loaded.note {
        log::warn!("settings: {note}");
    }
    let resources = app.path().resource_dir().ok();
    let config = Config {
        version: app.package_info().version.to_string(),
        settings: loaded.settings,
        settings_path,
        bundled_dir: catalog::bundled_dir(resources.as_deref()),
        user_dir: taktak_core::pack::registry::default_user_dir(),
        // `TAKTAK_NO_INPUT=1` runs without the keyboard listener and never asks for
        // permission (UI development and automated runs).
        listen: std::env::var_os("TAKTAK_NO_INPUT").is_none_or(|v| v.is_empty() || v == "0"),
    };
    log::info!(
        "TakTak {} starting; bundled packs: {}",
        config.version,
        config.bundled_dir.as_deref().map_or("none".into(), |d| d.display().to_string())
    );
    let service = Service::start(config, broadcaster(app.clone())?)?;

    // The OS decides whether TakTak launches at login (the user can remove it there too).
    match app.autolaunch().is_enabled() {
        Ok(on) => {
            service.update(|s| s.settings.launch_at_login = on);
        }
        Err(e) => log::warn!("cannot read the launch-at-login state: {e}"),
    }

    app.manage(hotkey::Hotkey::default());
    if let Some(accelerator) = service.settings().mute_hotkey
        && let Err(e) = hotkey::apply(app, Some(&accelerator))
    {
        // Keep the user's choice, but say it does nothing (Settings, and no menu shortcut).
        log::warn!("mute hotkey not registered: {e}");
        service.update(|s| s.mute_hotkey_error = Some(hotkey::startup_error(&e)));
    }

    let state = service.snapshot();
    app.manage(service);
    tray::create(app, &state)?;
    Ok(())
}

/// The service's [`Notify`]: hands each new state to the `taktak-events` thread, which emits
/// `state-changed` and updates the tray menu on the main thread. Only the newest of states
/// that pile up is sent. The thread blocks while nothing changes.
fn broadcaster<R: Runtime>(app: AppHandle<R>) -> std::io::Result<Notify> {
    let (tx, rx) = mpsc::channel::<AppState>();
    std::thread::Builder::new().name("taktak-events".into()).spawn(move || {
        while let Ok(mut state) = rx.recv() {
            if let Some(newest) = rx.try_iter().last() {
                state = newest;
            }
            if let Err(e) = app.emit(STATE_CHANGED, &state) {
                log::warn!("cannot emit {STATE_CHANGED}: {e}");
            }
            let handle = app.clone();
            if let Err(e) = app.run_on_main_thread(move || tray::sync(&handle, &state)) {
                log::warn!("cannot update the tray menu: {e}");
            }
        }
    })?;
    Ok(Box::new(move |_revision, state| {
        let _ = tx.send(state.clone());
    }))
}
