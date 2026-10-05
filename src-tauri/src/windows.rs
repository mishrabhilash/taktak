//! The webview windows (`docs/ui-contract.md` § Windows): the tray popover, Settings and the
//! onboarding window. All load the same bundle; the UI picks its view from the window label.
//!
//! Call these from a command or a spawned task, not directly from a synchronous event handler:
//! creating a webview there deadlocks on Windows (WebView2).
//!
//! Memory: each webview keeps WebKit helper processes alive (WebContent ~20–40 MB, plus the GPU
//! and Networking processes), so no window lives longer than it is useful. Settings and the
//! onboarding window are destroyed when they close. The popover is hidden on blur and destroyed
//! once it has stayed hidden for [`TRAY_KEEP`], or soon after another window opens; the next
//! click recreates it (~0.2 s).
//!
//! Also here: whether to offer the onboarding ([`offer_onboarding`], [`open_at_startup`]).

use crate::service::Service;
use crate::state::Permission;
use crate::webview;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, Runtime, WebviewWindow, WindowEvent};
use tauri_plugin_positioner::{Position, WindowExt};

/// The popover anchored to the tray icon.
pub const TRAY: &str = "tray";
/// The settings window.
pub const SETTINGS: &str = "settings";
/// The welcome and permission guide (M4).
pub const ONBOARDING: &str = "onboarding";

/// `onboarding.offer`: the onboarding was never closed, or the platform needs a permission the
/// user has not granted (live).
pub fn offer_onboarding(done: bool, permission_required: bool, permission: Permission) -> bool {
    !done || (permission_required && permission == Permission::Denied)
}

/// Whether the app opens the onboarding window at startup, once the first permission check is
/// done: when it is offered, or when a relaunch from the onboarding window asked for it again
/// (`--onboarding`).
pub fn open_at_startup(offer: bool, reopen: bool) -> bool {
    offer || reopen
}

/// Set once the app is quitting (or relaunching): closing the onboarding window then does not
/// count as the user closing it.
static EXITING: AtomicBool = AtomicBool::new(false);

/// Records that the app is quitting; see [`EXITING`].
pub fn mark_exiting() {
    EXITING.store(true, Ordering::Relaxed);
}

fn exiting() -> bool {
    EXITING.load(Ordering::Relaxed)
}

/// A tray click this soon after the popover hid on blur is the click that blurred it: it
/// closes the popover rather than reopening it.
const REOPEN_GUARD: Duration = Duration::from_millis(250);

/// How long the popover stays hidden before it is destroyed. Clicks in quick succession reuse
/// it; after that its WebKit processes are not worth keeping.
const TRAY_KEEP: Duration = Duration::from_secs(60);

/// How soon after Settings opens the hidden popover is destroyed, so two webviews (and two
/// WebContent processes) do not stay side by side. Not at once: Settings may have been opened
/// from the popover, whose command is still answering.
const TRAY_KEEP_BESIDE_SETTINGS: Duration = Duration::from_secs(1);

/// When the popover last hid because it lost focus.
static HIDDEN_ON_BLUR: Mutex<Option<Instant>> = Mutex::new(None);

/// Counts the times the popover was shown; a scheduled destroy only goes ahead if it was not
/// shown again since it was scheduled.
static TRAY_SHOWS: AtomicU64 = AtomicU64::new(0);

/// Held while a window is looked up and created or destroyed, so two quick requests (a double
/// click, the tray and a second launch at once) cannot both act on it. Only ever taken on
/// worker threads, never on the main thread that window creation waits for.
static OPENING: Mutex<()> = Mutex::new(());

fn hidden_recently(now: Instant) -> bool {
    HIDDEN_ON_BLUR
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .is_some_and(|at| now.saturating_duration_since(at) < REOPEN_GUARD)
}

/// Whether the popover was shown again since [`TRAY_SHOWS`] read `shows`.
fn shown_since(shows: u64) -> bool {
    TRAY_SHOWS.load(Ordering::Relaxed) != shows
}

/// Shows the settings window, focusing it if it exists and creating it otherwise. Closing it
/// destroys it (saves memory). A new window starts a new latency measurement.
pub fn show_settings<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let _opening = OPENING.lock().unwrap_or_else(PoisonError::into_inner);
    unhide(app);
    let window = match app.get_webview_window(SETTINGS) {
        Some(window) => {
            window.unminimize()?;
            window.show()?;
            window
        }
        None => {
            if let Some(service) = app.try_state::<Service>() {
                service.reset_latency();
            }
            webview::builder(app, SETTINGS)
                .title("TakTak Settings")
                .inner_size(820.0, 600.0)
                .min_inner_size(620.0, 460.0)
                .resizable(true)
                .center()
                .focused(true)
                .build()?
        }
    };
    if app.get_webview_window(TRAY).is_some() {
        schedule_tray_destroy(app, TRAY_KEEP_BESIDE_SETTINGS);
    }
    // An accessory app (no Dock icon) is not activated by opening a window; bring it forward.
    window.set_focus()
}

/// Shows the onboarding window, focusing it if it exists and creating it otherwise. Closing it
/// destroys it; the user closing it marks the onboarding as done (`settings.onboardingDone`),
/// quitting or relaunching while it is open does not.
pub fn show_onboarding<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let _opening = OPENING.lock().unwrap_or_else(PoisonError::into_inner);
    unhide(app);
    let window = match app.get_webview_window(ONBOARDING) {
        Some(window) => {
            window.unminimize()?;
            window.show()?;
            window
        }
        None => {
            let window = webview::builder(app, ONBOARDING)
                .title("Welcome to TakTak")
                .inner_size(480.0, 440.0)
                .resizable(false)
                .maximizable(false)
                .minimizable(false)
                .center()
                .focused(true)
                .build()?;
            let handle = app.clone();
            window.on_window_event(move |event| {
                if let WindowEvent::CloseRequested { .. } = event
                    && !exiting()
                    && let Some(service) = handle.try_state::<Service>()
                {
                    service.update(|s| s.settings.onboarding_done = true);
                }
            });
            window
        }
    };
    if app.get_webview_window(TRAY).is_some() {
        schedule_tray_destroy(app, TRAY_KEEP_BESIDE_SETTINGS);
    }
    window.set_focus()?;
    // So TakTak is in the Input Monitoring list even if the user opens System Settings by hand.
    crate::permission::request_for_onboarding(app);
    Ok(())
}

/// Closes the onboarding window if it is open (`finish_onboarding`, which has already marked
/// the onboarding as done).
pub fn close_onboarding<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    match app.get_webview_window(ONBOARDING) {
        Some(window) => window.destroy(),
        None => Ok(()),
    }
}

/// Whether the onboarding window is open (a relaunch then opens it again).
pub fn onboarding_open<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.get_webview_window(ONBOARDING).is_some()
}

/// Shows the tray popover under (or above) the tray icon, or hides it if it is showing.
pub fn toggle_tray<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let _opening = OPENING.lock().unwrap_or_else(PoisonError::into_inner);
    let window = match app.get_webview_window(TRAY) {
        Some(window) if window.is_visible()? => return hide_popover(app, &window),
        Some(_) if hidden_recently(Instant::now()) => {
            // This click blurred (and so hid) the popover: TakTak is still the active app.
            give_back_focus(app);
            return Ok(());
        }
        Some(window) => window,
        None => create_tray(app)?,
    };
    TRAY_SHOWS.fetch_add(1, Ordering::Relaxed);
    unhide(app);
    // Fails until the tray icon has reported its position; the window then opens where it is.
    if let Err(e) = window.move_window_constrained(Position::TrayCenter) {
        log::debug!("tray popover not positioned: {e}");
    }
    window.show()?;
    window.set_focus()
}

/// Hides the tray popover if it shows (Escape in the popover: the `hide_tray` command).
pub fn hide_tray<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    match app.get_webview_window(TRAY) {
        Some(window) => hide_popover(app, &window),
        None => Ok(()),
    }
}

/// Hides the popover, hands the keyboard back and schedules the popover's destruction.
fn hide_popover<R: Runtime>(app: &AppHandle<R>, popover: &WebviewWindow<R>) -> tauri::Result<()> {
    popover.hide()?;
    give_back_focus(app);
    schedule_tray_destroy(app, TRAY_KEEP);
    Ok(())
}

fn create_tray<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<WebviewWindow<R>> {
    let window = webview::builder(app, TRAY)
        .title("TakTak")
        .inner_size(340.0, 460.0)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible_on_all_workspaces(true)
        .visible(false)
        .build()?;
    let popover = window.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::Focused(false) = event {
            let app = popover.app_handle();
            match popover.is_visible() {
                // Another window or app took the focus, and with it the keyboard.
                Ok(true) => {
                    *HIDDEN_ON_BLUR.lock().unwrap_or_else(PoisonError::into_inner) =
                        Some(Instant::now());
                    let _ = popover.hide();
                }
                // Hidden while it had the focus (Escape): nothing else has the keyboard yet.
                Ok(false) => give_back_focus(app),
                Err(_) => return,
            }
            schedule_tray_destroy(app, TRAY_KEEP);
        }
    });
    Ok(window)
}

/// Destroys the popover after `after`, if it is hidden then and was not shown in between.
/// Sleeps on its own short-lived thread: nothing polls.
fn schedule_tray_destroy<R: Runtime>(app: &AppHandle<R>, after: Duration) {
    let shows = TRAY_SHOWS.load(Ordering::Relaxed);
    let app = app.clone();
    let spawned = thread::Builder::new().name("taktak-popover".into()).spawn(move || {
        thread::sleep(after);
        if let Err(e) = destroy_hidden_tray(&app, shows) {
            log::debug!("tray popover not destroyed: {e}");
        }
    });
    if let Err(e) = spawned {
        log::debug!("cannot schedule the tray popover's destruction: {e}");
    }
}

fn destroy_hidden_tray<R: Runtime>(app: &AppHandle<R>, shows: u64) -> tauri::Result<()> {
    let _opening = OPENING.lock().unwrap_or_else(PoisonError::into_inner);
    if shown_since(shows) {
        return Ok(());
    }
    match app.get_webview_window(TRAY) {
        Some(window) if !window.is_visible()? => {
            log::debug!("destroying the hidden tray popover");
            window.destroy()
        }
        _ => Ok(()),
    }
}

/// macOS: showing the popover made TakTak the active app, and hiding a window does not hand
/// that back. Hiding the app does: the app the user was typing in becomes active again. Not
/// while the settings or onboarding window shows (hiding the app would hide it too).
fn give_back_focus<R: Runtime>(app: &AppHandle<R>) {
    #[cfg(target_os = "macos")]
    {
        let window_shown = [SETTINGS, ONBOARDING].into_iter().any(|label| {
            app.get_webview_window(label).is_some_and(|window| window.is_visible().unwrap_or(false))
        });
        if !window_shown && let Err(e) = app.hide() {
            log::debug!("cannot hand the focus back: {e}");
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

/// macOS: undoes [`give_back_focus`] before a window shows (a hidden app's windows stay off
/// screen until it is unhidden).
fn unhide<R: Runtime>(app: &AppHandle<R>) {
    #[cfg(target_os = "macos")]
    if let Err(e) = app.show() {
        log::debug!("cannot unhide TakTak: {e}");
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_right_after_a_blur_hide_does_not_reopen() {
        let now = Instant::now();
        *HIDDEN_ON_BLUR.lock().unwrap() = Some(now);
        assert!(hidden_recently(now + Duration::from_millis(100)));
        assert!(!hidden_recently(now + REOPEN_GUARD));
        *HIDDEN_ON_BLUR.lock().unwrap() = None;
        assert!(!hidden_recently(now));
    }

    #[test]
    fn the_onboarding_is_offered_until_done_and_while_permission_is_missing() {
        use Permission::*;
        // First launch: offered whatever the permission.
        for permission in [Granted, Denied, Unknown] {
            assert!(offer_onboarding(false, true, permission));
            assert!(offer_onboarding(false, false, permission));
        }
        // Done: again only while a required permission is missing.
        assert!(offer_onboarding(true, true, Denied));
        assert!(!offer_onboarding(true, true, Granted));
        assert!(!offer_onboarding(true, true, Unknown), "TAKTAK_NO_INPUT or not checked yet");
        assert!(!offer_onboarding(true, false, Denied), "no permission step here");

        assert!(open_at_startup(true, false));
        assert!(open_at_startup(false, true), "relaunched from the onboarding window");
        assert!(!open_at_startup(false, false));
    }

    #[test]
    fn showing_the_popover_again_cancels_its_destruction() {
        let shows = TRAY_SHOWS.load(Ordering::Relaxed);
        assert!(!shown_since(shows), "still hidden: destroy it");
        TRAY_SHOWS.fetch_add(1, Ordering::Relaxed);
        assert!(shown_since(shows), "shown in between: keep it");
    }
}
