//! Global, listen-only keyboard hooks.
//!
//! Each platform backend captures exactly two things per event: which physical key, and
//! whether it went down or up. No characters, no text, no keyboard layout lookups.
//! The user callback runs on the hook thread and must return quickly (it only enqueues).

use crate::key::Key;

pub mod keymap_linux;
pub mod keymap_macos;
pub mod keymap_windows;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(target_os = "windows")]
use windows as platform;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyAction {
    Down,
    Up,
}

#[derive(Clone, Copy, Debug)]
pub struct KeyEvent {
    pub key: Key,
    pub action: KeyAction,
    /// When the OS says the event happened, on the [`crate::clock`] timebase.
    pub event_ns: u64,
    /// When our hook received it, on the [`crate::clock`] timebase.
    pub received_ns: u64,
}

#[derive(Debug)]
pub enum InputError {
    /// The OS refused the hook: on macOS Input Monitoring is not granted; on Linux (evdev
    /// backend) the keyboard devices in `/dev/input` are not readable. Never on Windows.
    PermissionDenied,
    Unsupported(&'static str),
    Platform(String),
}

impl std::fmt::Display for InputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InputError::PermissionDenied => f.write_str(PERMISSION_DENIED),
            InputError::Unsupported(why) => write!(f, "unsupported platform: {why}"),
            InputError::Platform(msg) => write!(f, "keyboard hook failed: {msg}"),
        }
    }
}

impl std::error::Error for InputError {}

#[cfg(not(target_os = "linux"))]
const PERMISSION_DENIED: &str = "keyboard listening was refused: grant Input Monitoring \
     permission (System Settings → Privacy & Security → Input Monitoring)";
#[cfg(target_os = "linux")]
const PERMISSION_DENIED: &str = "keyboard listening was refused: the keyboard devices in \
     /dev/input are not readable. On Wayland (or with TAKTAK_INPUT=evdev) TakTak reads them \
     directly, which needs membership of the `input` group: `sudo usermod -aG input \"$USER\"`, \
     then log out and back in. Note that this lets every program you run read every keystroke";

/// The longest plausible delay between an OS event timestamp and our hook receiving it. An
/// older timestamp means the two clocks disagree, and the receive time is used instead.
#[cfg(any(target_os = "linux", test))]
const MAX_EVENT_AGE_NS: u64 = 1_000_000_000;

/// `event_ns` for an event that the OS stamped `age_ns` before we received it at
/// `received_ns`, for backends whose timestamps are on another clock than [`crate::clock`].
#[cfg(any(target_os = "linux", test))]
fn backdate(received_ns: u64, age_ns: u64) -> u64 {
    if age_ns > MAX_EVENT_AGE_NS { received_ns } else { received_ns.saturating_sub(age_ns) }
}

/// A running listener. Dropping it stops the hook and joins its thread.
pub struct Listener {
    #[cfg(target_os = "macos")]
    inner: macos::TapHandle,
    #[cfg(target_os = "windows")]
    inner: windows::HookHandle,
    #[cfg(target_os = "linux")]
    inner: linux::Handle,
}

impl Listener {
    /// Times the OS made the hook miss events since the last call: macOS disabled the tap
    /// after a stall and it was re-enabled; Linux evdev dropped events (`SYN_DROPPED`).
    /// Always 0 on Windows, which removes a slow hook without telling. The hook thread cannot
    /// log, so poll this from the control side; keys pressed in that window made no sound.
    pub fn take_reenabled(&self) -> u32 {
        #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
        {
            self.inner.take_reenabled()
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            0
        }
    }
}

/// Starts listening on a dedicated thread. Auto-repeat and duplicate downs are already
/// filtered out: the callback sees exactly one `Down` and one `Up` per physical press.
pub fn start<F>(callback: F) -> Result<Listener, InputError>
where
    F: FnMut(KeyEvent) + Send + 'static,
{
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    {
        Ok(Listener { inner: platform::start(Box::new(callback))? })
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = callback;
        Err(InputError::Unsupported("no key listener for this operating system"))
    }
}

/// Whether the OS currently allows us to listen. Never prompts. macOS: Input Monitoring.
/// Windows: always (no permission exists). Linux: always on X11; with the evdev backend
/// (Wayland), whether the keyboard devices are readable.
pub fn has_permission() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::preflight()
    }
    #[cfg(target_os = "linux")]
    {
        linux::has_permission()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        true
    }
}

/// Asks the OS for permission, showing its prompt if it has not been answered before.
/// Returns whether permission is granted right now. Only macOS has a prompt; elsewhere this
/// is [`has_permission`].
pub fn request_permission() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::request()
    }
    #[cfg(target_os = "linux")]
    {
        linux::request_permission()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        true
    }
}

/// Tracks which keys are physically held, so OS auto-repeat (and any duplicate down events a
/// backend might emit) produce one sound per press, and stray ups without a down are dropped.
#[derive(Default)]
pub struct PressState {
    held: [u64; 4],
}

impl PressState {
    fn bit(key: Key) -> (usize, u64) {
        let i = key.index();
        (i / 64, 1u64 << (i % 64))
    }

    pub fn is_held(&self, key: Key) -> bool {
        let (w, b) = Self::bit(key);
        self.held[w] & b != 0
    }

    /// Returns `true` if this transition should produce a sound.
    pub fn accept(&mut self, key: Key, action: KeyAction) -> bool {
        let (w, b) = Self::bit(key);
        let was_held = self.held[w] & b != 0;
        match action {
            KeyAction::Down => {
                self.held[w] |= b;
                !was_held
            }
            KeyAction::Up => {
                self.held[w] &= !b;
                was_held
            }
        }
    }

    pub fn clear(&mut self) {
        self.held = [0; 4];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_repeat_plays_once() {
        let mut s = PressState::default();
        assert!(s.accept(Key::KeyJ, KeyAction::Down));
        assert!(!s.accept(Key::KeyJ, KeyAction::Down));
        assert!(!s.accept(Key::KeyJ, KeyAction::Down));
        assert!(s.accept(Key::KeyJ, KeyAction::Up));
        assert!(s.accept(Key::KeyJ, KeyAction::Down));
    }

    #[test]
    fn backdate_trusts_only_plausible_ages() {
        assert_eq!(backdate(10_000_000, 2_000_000), 8_000_000);
        assert_eq!(backdate(10_000_000, 0), 10_000_000);
        assert_eq!(backdate(1_000, 5_000), 0);
        assert_eq!(backdate(10_000_000_000, MAX_EVENT_AGE_NS + 1), 10_000_000_000);
    }

    #[test]
    fn stray_up_is_ignored_and_keys_are_independent() {
        let mut s = PressState::default();
        assert!(!s.accept(Key::Space, KeyAction::Up));
        assert!(s.accept(Key::ShiftLeft, KeyAction::Down));
        assert!(s.accept(Key::NonConvert, KeyAction::Down));
        assert!(s.is_held(Key::ShiftLeft) && s.is_held(Key::NonConvert));
        assert!(s.accept(Key::NonConvert, KeyAction::Up));
        assert!(s.is_held(Key::ShiftLeft));
    }
}
