//! Keyboard input for the app: the gate the hook checks before forwarding a key event, and
//! when to check Input Monitoring permission again.
//!
//! The hook callback runs on `taktak-input` and must never block, allocate, lock or log: it
//! loads one atomic and pushes into a wait-free ring. Nothing here looks at which key it was.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use taktak_core::audio::{Trigger, TriggerSender};
use taktak_core::input::{self, InputError, KeyEvent, Listener};

/// How often to check while permission is missing.
pub const POLL_DENIED: Duration = Duration::from_secs(2);
/// How often to check while granted (it can be revoked in System Settings).
pub const POLL_GRANTED: Duration = Duration::from_secs(10);

/// Whether key events should make sounds: the master switch is on and nothing muted them.
pub fn gate_open(enabled: bool, muted: bool) -> bool {
    enabled && !muted
}

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

/// Input Monitoring bookkeeping: asks the OS at most once per launch, then only polls.
#[derive(Debug)]
pub struct Permission {
    granted: bool,
    asked: bool,
    next_check: Instant,
}

impl Default for Permission {
    fn default() -> Self {
        Permission::new()
    }
}

impl Permission {
    pub fn new() -> Permission {
        Permission { granted: false, asked: false, next_check: Instant::now() }
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

    /// Checks with `has` (never prompts); the first time permission is missing, asks once
    /// with `request` (which may show the OS prompt). Returns whether it is granted now.
    pub fn check(
        &mut self,
        now: Instant,
        has: impl FnOnce() -> bool,
        request: impl FnOnce() -> bool,
    ) -> bool {
        let mut granted = has();
        if !granted && !self.asked {
            self.asked = true;
            granted = request();
        }
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
    fn gate_follows_enabled_and_muted() {
        assert!(gate_open(true, false));
        assert!(!gate_open(false, false));
        assert!(!gate_open(true, true));
        assert!(!gate_open(false, true));
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
    fn permission_is_requested_once_then_polled() {
        let mut p = Permission::new();
        let t0 = Instant::now();
        assert!(p.due(t0));
        let requests = Cell::new(0);
        let request = || {
            requests.set(requests.get() + 1);
            false
        };
        assert!(!p.check(t0, || false, request));
        assert_eq!(p.next_check(), t0 + POLL_DENIED);
        assert!(!p.due(t0 + Duration::from_secs(1)));
        // Later checks never prompt again.
        assert!(!p.check(t0 + POLL_DENIED, || false, request));
        assert!(!p.check(t0 + POLL_DENIED * 2, || false, request));
        assert_eq!(requests.get(), 1);
        // Granted in System Settings: picked up by the poll, then checked less often.
        let t1 = t0 + POLL_DENIED * 3;
        assert!(p.check(t1, || true, request));
        assert!(p.granted());
        assert_eq!(p.next_check(), t1 + POLL_GRANTED);
        assert_eq!(requests.get(), 1);
    }

    #[test]
    fn granted_at_launch_never_prompts() {
        let mut p = Permission::new();
        assert!(p.check(Instant::now(), || true, || panic!("must not prompt")));
    }
}
