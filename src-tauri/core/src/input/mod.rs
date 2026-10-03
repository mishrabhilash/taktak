//! Global, listen-only keyboard hooks.
//!
//! Each platform backend captures exactly two things per event: which physical key, and
//! whether it went down or up. No characters, no text, no keyboard layout lookups.
//! The user callback runs on the hook thread and must return quickly (it only enqueues).

use crate::key::Key;

pub mod keymap_macos;

#[cfg(target_os = "macos")]
mod macos;

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
    /// The OS refused the hook; on macOS this means Input Monitoring is not granted.
    PermissionDenied,
    Unsupported(&'static str),
    Platform(String),
}

impl std::fmt::Display for InputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InputError::PermissionDenied => f.write_str(
                "keyboard listening was refused: grant Input Monitoring permission \
                 (System Settings → Privacy & Security → Input Monitoring)",
            ),
            InputError::Unsupported(why) => write!(f, "unsupported platform: {why}"),
            InputError::Platform(msg) => write!(f, "keyboard hook failed: {msg}"),
        }
    }
}

impl std::error::Error for InputError {}

/// A running listener. Dropping it stops the hook and joins its thread.
pub struct Listener {
    #[cfg(target_os = "macos")]
    inner: macos::TapHandle,
}

impl Listener {
    /// Times the OS disabled the hook (macOS does after a stall) and it was re-enabled, since
    /// the last call. The hook thread cannot log, so poll this from the control side; keys
    /// pressed while the hook was off made no sound.
    pub fn take_reenabled(&self) -> u32 {
        #[cfg(target_os = "macos")]
        {
            self.inner.take_reenabled()
        }
        #[cfg(not(target_os = "macos"))]
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
    #[cfg(target_os = "macos")]
    {
        Ok(Listener { inner: macos::start(Box::new(callback))? })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = callback;
        Err(InputError::Unsupported("Windows and Linux listeners are not implemented yet"))
    }
}

/// Whether the OS currently allows us to listen (macOS: Input Monitoring). Never prompts.
pub fn has_permission() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::preflight()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

/// Asks the OS for permission, showing its prompt if it has not been answered before.
/// Returns whether permission is granted right now.
pub fn request_permission() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::request()
    }
    #[cfg(not(target_os = "macos"))]
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
