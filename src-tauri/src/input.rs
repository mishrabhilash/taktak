//! Keyboard input for the app: forwarding a key event through the gate the hook checks (its
//! value is [`crate::rules::gate_open`]), and when to check Input Monitoring permission again.
//!
//! The hook callback runs on `taktak-input` and must never block, allocate, lock or log: it
//! loads one atomic, notes when a key went down ([`Activity::key_down`], which wakes the control
//! thread only when that key press ends an idle sleep) and pushes into a wait-free ring. Nothing
//! here looks at which key it was.
//!
//! Checking never prompts. Asking macOS to list TakTak is [`crate::permission`]'s job: the
//! onboarding window's button (`open_permission_settings`), and once when that window appears.

use crate::idle::Activity;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};
use taktak_core::audio::{Trigger, TriggerSender};
use taktak_core::input::{self, InputError, KeyAction, KeyEvent, Listener};

/// How often to check while permission is missing.
pub const POLL_DENIED: Duration = Duration::from_secs(2);
/// How often to check while granted (it can be revoked in System Settings).
pub const POLL_GRANTED: Duration = Duration::from_secs(10);

/// The hook's whole job per event: if the gate is open, note a key-down in `activity` and hand
/// the event to `send`. Returns whether it was sent (`false` also when the ring was full).
#[inline]
pub fn forward(
    gate: &AtomicBool,
    activity: &Activity,
    event: KeyEvent,
    send: impl FnOnce(Trigger) -> bool,
) -> bool {
    if !gate.load(Ordering::Relaxed) {
        return false;
    }
    if event.action == KeyAction::Down {
        activity.key_down(event.received_ns);
    }
    send(Trigger::from(event))
}

/// Where key events come from.
#[derive(Clone, Default)]
pub enum KeySource {
    /// The OS hook ([`taktak_core::input`]).
    #[default]
    Os,
    /// The self-test's keyboard: presses go through the same [`forward`] as the OS hook's.
    Synthetic(Arc<SyntheticKeys>),
}

/// A running key listener.
pub enum Hook {
    Os(Listener),
    Synthetic(SyntheticAttached),
}

impl Hook {
    /// See [`Listener::take_reenabled`]; always 0 for the synthetic keyboard.
    pub fn take_reenabled(&self) -> u32 {
        match self {
            Hook::Os(listener) => listener.take_reenabled(),
            Hook::Synthetic(_) => 0,
        }
    }
}

type Forward = Box<dyn FnMut(KeyEvent) + Send>;

/// The self-test's keyboard (`KeySource::Synthetic`): no OS hook and no permission, but the
/// app's real gate, activity and trigger ring.
#[derive(Default)]
pub struct SyntheticKeys {
    hook: Mutex<Option<Forward>>,
}

impl SyntheticKeys {
    /// Presses or releases a key now. `false` while no listener runs (nothing listens).
    pub fn press(&self, action: KeyAction) -> bool {
        let mut hook = self.hook.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(forward) = hook.as_mut() else { return false };
        let now = taktak_core::clock::now_ns();
        forward(KeyEvent {
            key: taktak_core::key::Key::KeyA,
            action,
            event_ns: now,
            received_ns: now,
        });
        true
    }

    /// Whether a listener runs (the app would hear a key press now).
    pub fn listening(&self) -> bool {
        self.hook.lock().unwrap_or_else(PoisonError::into_inner).is_some()
    }
}

/// The synthetic keyboard's listener: detaches when dropped.
pub struct SyntheticAttached(Arc<SyntheticKeys>);

impl Drop for SyntheticAttached {
    fn drop(&mut self) {
        *self.0.hook.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }
}

/// Starts the listener for `source`, forwarding through `gate` into `sender`.
pub fn start_listener(
    source: &KeySource,
    gate: Arc<AtomicBool>,
    activity: Arc<Activity>,
    mut sender: TriggerSender,
) -> Result<Hook, InputError> {
    let forward_event = move |event: KeyEvent| {
        forward(&gate, &activity, event, |trigger| sender.send(trigger));
    };
    match source {
        KeySource::Os => input::start(forward_event).map(Hook::Os),
        KeySource::Synthetic(keys) => {
            *keys.hook.lock().unwrap_or_else(PoisonError::into_inner) =
                Some(Box::new(forward_event));
            Ok(Hook::Synthetic(SyntheticAttached(keys.clone())))
        }
    }
}

/// Input Monitoring bookkeeping: polls, never prompts.
#[derive(Debug)]
pub struct Permission {
    granted: bool,
    /// Whether [`Permission::check`] ran at least once.
    checked: bool,
    next_check: Instant,
}

impl Default for Permission {
    fn default() -> Self {
        Permission::new()
    }
}

impl Permission {
    pub fn new() -> Permission {
        Permission { granted: false, checked: false, next_check: Instant::now() }
    }

    pub fn granted(&self) -> bool {
        self.granted
    }

    pub fn due(&self, now: Instant) -> bool {
        now >= self.next_check
    }

    pub fn next_check(&self) -> Instant {
        self.next_check
    }

    /// Whether this is before the first [`Permission::check`].
    pub fn first(&self) -> bool {
        !self.checked
    }

    /// Checks with `has` (which must never prompt) and returns whether it is granted now.
    pub fn check(&mut self, now: Instant, has: impl FnOnce() -> bool) -> bool {
        let granted = has();
        self.checked = true;
        self.set(now, granted);
        granted
    }

    /// Records a verdict from elsewhere (the hook refused to start) and schedules the next check.
    pub fn set(&mut self, now: Instant, granted: bool) {
        self.granted = granted;
        self.next_check = now + if granted { POLL_GRANTED } else { POLL_DENIED };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::idle::Waker;
    use std::cell::Cell;
    use taktak_core::key::Key;

    fn event() -> KeyEvent {
        KeyEvent { key: Key::KeyA, action: KeyAction::Down, event_ns: 1, received_ns: 2 }
    }

    #[test]
    fn closed_gate_drops_events_without_sending() {
        let gate = AtomicBool::new(false);
        let activity = Activity::new(Arc::new(Waker::default()));
        let sent = Cell::new(0);
        let send = |t: Trigger| {
            assert_eq!((t.event_ns, t.received_ns), (1, 2));
            sent.set(sent.get() + 1);
            true
        };
        assert!(!forward(&gate, &activity, event(), send));
        assert_eq!(sent.get(), 0);
        assert_eq!(activity.last_key_ns(), 0, "a silent key is no activity");
        gate.store(true, Ordering::Relaxed);
        assert!(forward(&gate, &activity, event(), send));
        assert_eq!(sent.get(), 1);
        assert_eq!(activity.last_key_ns(), 2, "the receive time");
        // A full ring is reported, not retried.
        assert!(!forward(&gate, &activity, event(), |_| false));
        // A key-up is no new activity.
        let up = KeyEvent { action: KeyAction::Up, received_ns: 9, ..event() };
        assert!(forward(&gate, &activity, up, |_| true));
        assert_eq!(activity.last_key_ns(), 2);
    }

    #[test]
    fn the_synthetic_keyboard_uses_the_real_path() {
        let keys = Arc::new(SyntheticKeys::default());
        let source = KeySource::Synthetic(keys.clone());
        assert!(!keys.press(KeyAction::Down), "nothing listens yet");
        let gate = Arc::new(AtomicBool::new(true));
        let activity = Arc::new(Activity::new(Arc::new(Waker::default())));
        let (sender, mut receiver) = taktak_core::audio::trigger_ring();
        let hook = start_listener(&source, gate.clone(), activity.clone(), sender).unwrap();
        assert!(keys.listening() && keys.press(KeyAction::Down) && keys.press(KeyAction::Up));
        assert_eq!(receiver.len(), 2);
        assert!(activity.last_key_ns() > 0);
        gate.store(false, Ordering::Relaxed);
        assert!(keys.press(KeyAction::Down), "heard, but the gate drops it");
        assert_eq!(receiver.len(), 2);
        assert_eq!(hook.take_reenabled(), 0);
        drop(hook);
        assert!(!keys.listening());
        assert_eq!(receiver.discard_before(u64::MAX), 2);
    }

    #[test]
    fn permission_is_polled_and_never_requested() {
        let mut p = Permission::new();
        let t0 = Instant::now();
        assert!(p.due(t0));
        assert!(p.first());
        assert!(!p.check(t0, || false));
        assert!(!p.first());
        assert_eq!(p.next_check(), t0 + POLL_DENIED);
        assert!(!p.due(t0 + Duration::from_secs(1)));
        assert!(!p.check(t0 + POLL_DENIED, || false));
        // Granted in System Settings: picked up by the poll, then checked less often.
        let t1 = t0 + POLL_DENIED * 2;
        assert!(p.check(t1, || true));
        assert!(p.granted());
        assert_eq!(p.next_check(), t1 + POLL_GRANTED);
        // The hook refused to start: counted as denied, checked again soon.
        p.set(t1, false);
        assert!(!p.granted());
        assert_eq!(p.next_check(), t1 + POLL_DENIED);
    }
}
