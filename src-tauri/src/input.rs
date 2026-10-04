//! Keyboard input for the app: forwarding a key event through the gate the hook checks (its
//! value is [`crate::rules::gate_open`]), and when to check Input Monitoring permission again.
//!
//! The hook callback runs on `taktak-input` and must never block, allocate, lock or log: it
//! loads one atomic and pushes into a wait-free ring. Nothing here looks at which key it was.
//!
//! Checking never prompts. Asking macOS to list TakTak is [`crate::permission`]'s job: the
//! onboarding window's button (`open_permission_settings`), and once when that window appears.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use taktak_core::audio::{Trigger, TriggerSender};
use taktak_core::input::{self, InputError, KeyEvent, Listener};

/// How often to check while permission is missing.
pub const POLL_DENIED: Duration = Duration::from_secs(2);
/// How often to check while granted (it can be revoked in System Settings).
pub const POLL_GRANTED: Duration = Duration::from_secs(10);

/// The hook's whole job per event: if the gate is open, hand the event to `send`. Returns
/// whether it was sent (`false` also when the ring was full).
#[inline]
pub fn forward(gate: &AtomicBool, event: KeyEvent, send: impl FnOnce(Trigger) -> bool) -> bool {
    gate.load(Ordering::Relaxed) && send(Trigger::from(event))
}

/// Starts the OS listener, forwarding through `gate` into `sender`.
pub fn start_listener(
    gate: Arc<AtomicBool>,
    mut sender: TriggerSender,
) -> Result<Listener, InputError> {
    input::start(move |event| {
        forward(&gate, event, |trigger| sender.send(trigger));
    })
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
    use std::cell::Cell;
    use taktak_core::input::KeyAction;
    use taktak_core::key::Key;

    fn event() -> KeyEvent {
        KeyEvent { key: Key::KeyA, action: KeyAction::Down, event_ns: 1, received_ns: 2 }
    }

    #[test]
    fn closed_gate_drops_events_without_sending() {
        let gate = AtomicBool::new(false);
        let sent = Cell::new(0);
        let send = |t: Trigger| {
            assert_eq!((t.event_ns, t.received_ns), (1, 2));
            sent.set(sent.get() + 1);
            true
        };
        assert!(!forward(&gate, event(), send));
        assert_eq!(sent.get(), 0);
        gate.store(true, Ordering::Relaxed);
        assert!(forward(&gate, event(), send));
        assert_eq!(sent.get(), 1);
        // A full ring is reported, not retried.
        assert!(!forward(&gate, event(), |_| false));
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
