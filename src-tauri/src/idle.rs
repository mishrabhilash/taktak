//! Idle sleep: closing the output stream after `settings.idleSleepMinutes` without a key press,
//! and reopening it on the next key-down.
//!
//! An open output stream keeps the audio device awake, which costs `coreaudiod` 5–9 % of a
//! core even in silence. While keys could sound but nobody types, the control thread closes the
//! stream and keeps only the key listener (and the decoded pack). The next key-down ends the
//! sleep: the hook marks the time, flips [`Activity`]'s `asleep` flag and wakes the control
//! thread ([`Waker`]), which reopens the stream; the key press waits in the trigger ring (which
//! outlives the stream) and plays in the new stream's first buffer.
//!
//! The hook's part ([`Activity::key_down`]) is two atomic stores, one atomic load and, only for
//! the key-down that ends a sleep, one swap and a [`Thread::unpark`]: no allocation, no lock, no
//! blocking. The idle timer is a deadline the control thread computes when it next decides
//! what runs ([`IdleSleep::deadline_ns`]); it adds no periodic wake-up.
//!
//! Nothing here knows which key was pressed: only when.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread::{self, Thread};
use std::time::Duration;

/// The default `settings.idleSleepMinutes`.
pub const DEFAULT_MINUTES: u32 = 5;
/// The longest `settings.idleSleepMinutes` (a day); larger values are limited to it.
pub const MAX_MINUTES: u32 = 24 * 60;
/// One `idleSleepMinutes` unit in the app (the self-test uses a shorter one).
pub const MINUTE: Duration = Duration::from_secs(60);

/// How long the output may stay open without a key press, for `minutes`
/// (`settings.idleSleepMinutes`, one unit being `minute`); `None` = never sleep (0).
pub fn idle_after(minutes: u32, minute: Duration) -> Option<Duration> {
    (minutes > 0).then(|| minute.saturating_mul(minutes.min(MAX_MINUTES)))
}

/// `d` for the log: "5 min", or "300 ms" for the self-test's short idle times.
pub fn describe(d: Duration) -> String {
    if d.as_secs() >= 60 && d.as_secs().is_multiple_of(60) && d.subsec_nanos() == 0 {
        format!("{} min", d.as_secs() / 60)
    } else {
        format!("{} ms", d.as_millis())
    }
}

/// `settings.idleSleepMinutes` from a UI value: rounded, limited to `0..=MAX_MINUTES`;
/// non-finite → [`DEFAULT_MINUTES`].
pub fn minutes_from(value: f64) -> u32 {
    if value.is_finite() {
        value.round().clamp(0.0, f64::from(MAX_MINUTES)) as u32
    } else {
        DEFAULT_MINUTES
    }
}

/// Ends the control thread's `thread::park` from any thread, without blocking or allocating.
#[derive(Debug, Default)]
pub struct Waker {
    thread: OnceLock<Thread>,
}

impl Waker {
    /// Makes the calling thread the one [`Waker::wake`] wakes (the first call wins).
    pub fn register_current(&self) {
        let _ = self.thread.set(thread::current());
    }

    /// Ends the registered thread's current park, or its next one. A no-op before
    /// [`Waker::register_current`] (a message sent then is picked up on its first round anyway).
    pub fn wake(&self) {
        if let Some(thread) = self.thread.get() {
            thread.unpark();
        }
    }
}

/// What the key hook shares with the control thread about typing: when the last key went down,
/// and whether the output sleeps waiting for one.
#[derive(Debug)]
pub struct Activity {
    /// When the last key went down (the hook's receive time, `clock::now_ns`); 0 = never.
    last_key_ns: AtomicU64,
    /// Set by the control thread while the output sleeps; the next key-down clears it and wakes
    /// the control thread.
    asleep: AtomicBool,
    waker: Arc<Waker>,
}

impl Activity {
    pub fn new(waker: Arc<Waker>) -> Activity {
        Activity { last_key_ns: AtomicU64::new(0), asleep: AtomicBool::new(false), waker }
    }

    /// The hook saw a key go down at `received_ns` (gate open). Runs on the input thread: atomics
    /// and, for the key-down that ends a sleep, one unpark. Nothing else.
    #[inline]
    pub fn key_down(&self, received_ns: u64) {
        // SeqCst pairs with `IdleSleep::update`: either the control thread sees this time after
        // it arms the flag, or this load sees the flag and wakes it.
        self.last_key_ns.store(received_ns, Ordering::SeqCst);
        if self.asleep.load(Ordering::SeqCst) && self.asleep.swap(false, Ordering::SeqCst) {
            self.waker.wake();
        }
    }

    /// When the last key went down (0 = never).
    pub fn last_key_ns(&self) -> u64 {
        self.last_key_ns.load(Ordering::SeqCst)
    }

    fn arm(&self, asleep: bool) {
        self.asleep.store(asleep, Ordering::SeqCst);
    }

    fn armed(&self) -> bool {
        self.asleep.load(Ordering::SeqCst)
    }
}

/// The control thread's side of idle sleep.
#[derive(Debug)]
pub struct IdleSleep {
    activity: Arc<Activity>,
    /// Activity the hook does not see: keys became able to sound, the output opened for them,
    /// or the setting changed (`clock::now_ns`).
    since_ns: u64,
    asleep: bool,
}

impl IdleSleep {
    pub fn new(activity: Arc<Activity>, now_ns: u64) -> IdleSleep {
        IdleSleep { activity, since_ns: now_ns, asleep: false }
    }

    /// Whether the output sleeps now (closed until the next key-down).
    pub fn asleep(&self) -> bool {
        self.asleep
    }

    /// Counts as activity: the idle time starts again from `now_ns`.
    pub fn touch(&mut self, now_ns: u64) {
        self.since_ns = self.since_ns.max(now_ns);
    }

    fn last_activity_ns(&self) -> u64 {
        self.since_ns.max(self.activity.last_key_ns())
    }

    fn idle(&self, now_ns: u64, after: Duration) -> bool {
        now_ns.saturating_sub(self.last_activity_ns()) >= duration_ns(after)
    }

    /// When the output should go to sleep if no key goes down before (`clock::now_ns`); `None`
    /// while it sleeps already or never does (`after` is `None`).
    pub fn deadline_ns(&self, after: Option<Duration>) -> Option<u64> {
        let after = after.filter(|_| !self.asleep)?;
        Some(self.last_activity_ns().saturating_add(duration_ns(after)))
    }

    /// Decides whether the output sleeps at `now_ns`, given whether a key press could make a
    /// sound (`keys_wanted`) and the idle time (`after`, `None` = never). Starting a sleep arms
    /// the hook's wake-up; a key-down since then (the hook disarmed it) ends the sleep. Returns
    /// the new [`IdleSleep::asleep`].
    pub fn update(&mut self, keys_wanted: bool, after: Option<Duration>, now_ns: u64) -> bool {
        let Some(after) = after.filter(|_| keys_wanted) else {
            self.wake();
            return false;
        };
        if self.asleep {
            if self.activity.armed() {
                return true;
            }
            // The hook saw a key go down.
            self.asleep = false;
            return false;
        }
        if !self.idle(now_ns, after) {
            return false;
        }
        // Arm first, then look again: a key-down racing with this is either seen here, or sees
        // the flag and wakes the control thread (which then finds the flag cleared).
        self.activity.arm(true);
        if self.idle(now_ns, after) {
            self.asleep = true;
        } else {
            self.activity.arm(false);
        }
        self.asleep
    }

    /// Ends a sleep without a key press (sounds turned off, a preview, the setting changed).
    fn wake(&mut self) {
        self.activity.arm(false);
        self.asleep = false;
    }
}

fn duration_ns(d: Duration) -> u64 {
    u64::try_from(d.as_nanos()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    const MS: u64 = 1_000_000;
    const AFTER: Option<Duration> = Some(Duration::from_millis(100));

    fn rig(now_ns: u64) -> (Arc<Activity>, IdleSleep) {
        let activity = Arc::new(Activity::new(Arc::new(Waker::default())));
        let idle = IdleSleep::new(activity.clone(), now_ns);
        (activity, idle)
    }

    #[test]
    fn minutes_map_to_an_idle_time() {
        assert_eq!(idle_after(0, MINUTE), None, "0 = never");
        assert_eq!(idle_after(5, MINUTE), Some(Duration::from_secs(300)));
        assert_eq!(idle_after(u32::MAX, MINUTE), Some(MINUTE * MAX_MINUTES), "limited");
        assert_eq!(idle_after(3, Duration::from_millis(100)), Some(Duration::from_millis(300)));
        assert_eq!(describe(Duration::from_secs(300)), "5 min");
        assert_eq!(describe(Duration::from_millis(300)), "300 ms");
        assert_eq!(minutes_from(4.6), 5);
        assert_eq!(minutes_from(-3.0), 0);
        assert_eq!(minutes_from(1e9), MAX_MINUTES);
        assert_eq!(minutes_from(f64::NAN), DEFAULT_MINUTES);
    }

    #[test]
    fn sleeps_after_the_idle_time_and_a_key_down_wakes_it() {
        let (activity, mut idle) = rig(1_000 * MS);
        assert_eq!(idle.deadline_ns(AFTER), Some(1_100 * MS));
        assert!(!idle.update(true, AFTER, 1_099 * MS), "not yet");
        assert!(idle.update(true, AFTER, 1_100 * MS));
        assert!(idle.asleep() && activity.armed());
        assert_eq!(idle.deadline_ns(AFTER), None, "asleep: no deadline");
        assert!(idle.update(true, AFTER, 5_000 * MS), "stays asleep without a key");

        activity.key_down(5_001 * MS);
        assert!(!activity.armed(), "the hook disarms it");
        assert!(!idle.update(true, AFTER, 5_002 * MS), "awake");
        assert_eq!(idle.deadline_ns(AFTER), Some(5_101 * MS), "counted from the key press");
        assert!(!idle.update(true, AFTER, 5_100 * MS));
        assert!(idle.update(true, AFTER, 5_101 * MS), "and asleep again without another");
    }

    #[test]
    fn typing_keeps_it_awake() {
        let (activity, mut idle) = rig(0);
        for t in (50..1_000).step_by(50) {
            activity.key_down(t * MS);
            assert!(!idle.update(true, AFTER, t * MS + 10 * MS), "at {t} ms");
        }
        assert!(!activity.armed(), "never armed while typing");
    }

    #[test]
    fn never_sleeps_when_off_or_when_keys_cannot_sound() {
        let (activity, mut idle) = rig(0);
        assert!(!idle.update(true, None, u64::MAX), "idleSleepMinutes 0");
        assert_eq!(idle.deadline_ns(None), None);
        assert!(!idle.update(false, AFTER, u64::MAX), "muted: the output closes anyway");
        // Asleep, then sounds go off: the sleep ends and the hook is disarmed.
        assert!(idle.update(true, AFTER, 200 * MS));
        assert!(!idle.update(false, AFTER, 300 * MS));
        assert!(!idle.asleep() && !activity.armed());
        // Asleep, then the setting goes to 0: awake (the output reopens).
        assert!(idle.update(true, AFTER, 400 * MS));
        assert!(!idle.update(true, None, 500 * MS));
        assert!(!activity.armed());
    }

    #[test]
    fn touching_restarts_the_idle_time() {
        let (_activity, mut idle) = rig(0);
        idle.touch(90 * MS);
        assert!(!idle.update(true, AFTER, 150 * MS));
        assert_eq!(idle.deadline_ns(AFTER), Some(190 * MS));
        idle.touch(10 * MS);
        assert_eq!(idle.deadline_ns(AFTER), Some(190 * MS), "never moves back");
        assert!(idle.update(true, AFTER, 190 * MS));
    }

    #[test]
    fn a_key_down_racing_the_decision_is_never_lost() {
        // The key goes down between the first idle check and arming: the second look sees it.
        let (activity, mut idle) = rig(0);
        activity.key_down(150 * MS);
        assert!(!idle.update(true, AFTER, 160 * MS));
        assert!(!activity.armed(), "disarmed again");
        // The key goes down after arming: the hook sees the flag and wakes the control thread.
        let woken = Arc::new(Waker::default());
        woken.register_current();
        let activity = Arc::new(Activity::new(woken));
        let mut idle = IdleSleep::new(activity.clone(), 0);
        assert!(idle.update(true, AFTER, 200 * MS));
        let hook = {
            let activity = activity.clone();
            thread::spawn(move || activity.key_down(201 * MS))
        };
        hook.join().unwrap();
        // The unpark token is set: this park returns at once instead of after 10 s.
        let started = Instant::now();
        thread::park_timeout(Duration::from_secs(10));
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(!idle.update(true, AFTER, 202 * MS));
    }

    #[test]
    fn only_the_key_down_that_ends_a_sleep_wakes_the_thread() {
        let (activity, mut idle) = rig(0);
        activity.key_down(10 * MS);
        assert!(!activity.armed());
        assert!(idle.update(true, AFTER, 500 * MS));
        activity.key_down(501 * MS);
        activity.key_down(502 * MS);
        assert!(!activity.armed());
        assert_eq!(activity.last_key_ns(), 502 * MS);
    }
}
