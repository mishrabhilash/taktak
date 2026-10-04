//! Asking macOS to list TakTak under Privacy & Security → Input Monitoring: from
//! `open_permission_settings` (the onboarding window's "Allow Input Monitoring", and the same
//! button in the popover and Settings), and once per launch when the onboarding window first
//! appears, so TakTak is in the list even if the user opens System Settings by hand.
//!
//! How macOS behaves (macOS 26, seen in the `tccd` log; see docs/platform-notes.md § macOS):
//! the request (`IOHIDRequestAccess`) returns "denied" at once, and macOS's alert agent
//! (`universalAccessAuthWarn`) then shows "TakTak would like to receive keystrokes…" and adds
//! TakTak to the list, switched off, a few tens of milliseconds later. Once the agent has
//! recorded the app (outside the TCC database) it stays silent, also after `tccutil reset`, so a
//! new request may leave no trace: TakTak then has to be added with + (the window says so).
//!
//! Hence: ask on the main thread, wait for the entry to appear before opening the pane (so the
//! pane lists TakTak and does not race the alert), and tell the UI whether TakTak is listed.
//! Checking ([`input::access`]) never prompts. Nothing here asks in a loop: the automatic
//! request runs at most once per launch, and only while TakTak has never been decided on.

use crate::service::Service;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use taktak_core::input::{self, Access};
use tauri::{AppHandle, Manager, Runtime};

/// How often to look for the entry while it settles.
const SETTLE_POLL: Duration = Duration::from_millis(50);
/// Polls per request before giving up on the entry (1 s).
const SETTLE_STEPS: u32 = 20;
/// How long after the onboarding window appears its automatic request runs, so the window is on
/// screen (and explains) before macOS's alert does.
const ONBOARDING_DELAY: Duration = Duration::from_millis(700);

/// Set once TakTak has asked in this launch (the automatic request then never runs).
static REQUESTED: AtomicBool = AtomicBool::new(false);

/// What `open_permission_settings` does after asking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// Open the Input Monitoring pane: always, unless the request itself just granted access.
    pub open_pane: bool,
    /// TakTak is in the Input Monitoring list (always true where there is no list).
    pub listed: bool,
}

/// Ask only when macOS has never decided: a request for an app that is listed (on or off)
/// shows nothing.
pub fn should_request(before: Access) -> bool {
    before == Access::Undetermined
}

/// The onboarding window's automatic request: where a permission is required, not yet asked in
/// this launch, and never decided.
pub fn should_auto_request(required: bool, before: Access, already_asked: bool) -> bool {
    required && !already_asked && should_request(before)
}

pub fn outcome(before: Access, after: Access) -> Outcome {
    let just_granted = before != Access::Granted && after == Access::Granted;
    Outcome { open_pane: !just_granted, listed: after != Access::Undetermined }
}

/// Checks until the answer is no longer undetermined, waiting `wait` between up to `steps`
/// further checks.
fn settle(mut check: impl FnMut() -> Access, steps: u32, mut wait: impl FnMut()) -> Access {
    let mut access = check();
    for _ in 0..steps {
        if access != Access::Undetermined {
            break;
        }
        wait();
        access = check();
    }
    access
}

/// Runs `task` on the main thread and waits for it. Must not be called on the main thread.
fn on_main_blocking<R: Runtime, T: Send + 'static>(
    app: &AppHandle<R>,
    task: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let (tx, rx) = mpsc::channel();
    if let Err(e) = app.run_on_main_thread(move || {
        let _ = tx.send(task());
    }) {
        log::warn!("cannot reach the main thread to ask for Input Monitoring: {e}");
        return None;
    }
    rx.recv().ok()
}

/// Asks on the main thread and waits up to 1 s for macOS to record it; if it left no trace,
/// asks once more through CoreGraphics. Returns where things stand then. Blocks.
fn request_and_settle<R: Runtime>(app: &AppHandle<R>) -> Access {
    REQUESTED.store(true, Ordering::Relaxed);
    let wait = || thread::sleep(SETTLE_POLL);
    on_main_blocking(app, input::request_permission);
    let mut after = settle(input::access, SETTLE_STEPS, wait);
    if input::needs_fallback_request(after) {
        log::debug!("the Input Monitoring request left no trace; asking through CoreGraphics");
        on_main_blocking(app, input::request_permission_fallback);
        after = settle(input::access, SETTLE_STEPS, wait);
    }
    if after == Access::Undetermined {
        log::warn!(
            "macOS did not add TakTak to the Input Monitoring list (its alert agent stays silent \
             once it has recorded an app); TakTak has to be added there with +"
        );
    }
    after
}

/// `open_permission_settings`, before the pane opens: asks if macOS has never decided, and
/// says whether to open the pane and whether TakTak is listed. Blocks for up to about 2 s;
/// call it off the main thread.
pub fn ask<R: Runtime>(app: &AppHandle<R>) -> Outcome {
    let before = input::access();
    let after = if should_request(before) { request_and_settle(app) } else { before };
    log::debug!("Input Monitoring: {before:?} before asking, {after:?} after");
    outcome(before, after)
}

/// Called whenever the onboarding window is shown: the first time in a launch (and only while
/// macOS has never decided on TakTak), asks macOS to list TakTak, shortly after the window
/// appears. Returns at once.
pub fn request_for_onboarding<R: Runtime>(app: &AppHandle<R>) {
    let required =
        app.try_state::<Service>().is_some_and(|s| s.snapshot().onboarding.permission_required);
    if !required || REQUESTED.load(Ordering::Relaxed) {
        return;
    }
    let app = app.clone();
    let spawned = thread::Builder::new().name("taktak-permission".into()).spawn(move || {
        thread::sleep(ONBOARDING_DELAY);
        let before = input::access();
        if should_auto_request(required, before, REQUESTED.swap(true, Ordering::Relaxed)) {
            let after = request_and_settle(&app);
            log::info!("asked macOS to list TakTak under Input Monitoring: {after:?}");
        }
    });
    if let Err(e) = spawned {
        log::warn!("cannot start the Input Monitoring request: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Access::{Denied, Granted, Undetermined};

    #[test]
    fn asks_only_while_macos_has_never_decided() {
        assert!(should_request(Undetermined));
        assert!(!should_request(Denied), "listed and off: asking shows nothing");
        assert!(!should_request(Granted));
    }

    #[test]
    fn the_automatic_request_runs_at_most_once_and_only_where_required() {
        assert!(should_auto_request(true, Undetermined, false));
        assert!(!should_auto_request(true, Undetermined, true), "already asked this launch");
        assert!(!should_auto_request(false, Undetermined, false), "no permission step");
        assert!(!should_auto_request(true, Denied, false));
        assert!(!should_auto_request(true, Granted, false));
    }

    #[test]
    fn the_pane_opens_unless_the_request_granted_access() {
        // Listed by the request (the alert shows): open the pane, which now lists TakTak.
        assert_eq!(outcome(Undetermined, Denied), Outcome { open_pane: true, listed: true });
        // macOS left no trace: open the pane anyway; the window explains +.
        assert_eq!(outcome(Undetermined, Undetermined), Outcome { open_pane: true, listed: false });
        // Already listed and off: straight to the pane.
        assert_eq!(outcome(Denied, Denied), Outcome { open_pane: true, listed: true });
        // Granted by the request (managed Macs): nothing to open.
        assert_eq!(outcome(Undetermined, Granted), Outcome { open_pane: false, listed: true });
        assert_eq!(outcome(Denied, Granted), Outcome { open_pane: false, listed: true });
        // Granted all along (e.g. a refused listener): the user asked for the pane.
        assert_eq!(outcome(Granted, Granted), Outcome { open_pane: true, listed: true });
    }

    #[test]
    fn settling_waits_for_the_entry_then_stops() {
        let answers = [Undetermined, Undetermined, Denied, Granted];
        let (mut i, mut waits) = (0, 0);
        let got = settle(
            || {
                i += 1;
                answers[i - 1]
            },
            10,
            || waits += 1,
        );
        assert_eq!((got, waits), (Denied, 2));

        let mut waits = 0;
        assert_eq!(settle(|| Undetermined, 3, || waits += 1), Undetermined);
        assert_eq!(waits, 3, "gives up after the steps");

        let mut waits = 0;
        assert_eq!(settle(|| Granted, 3, || waits += 1), Granted);
        assert_eq!(waits, 0);
    }
}
