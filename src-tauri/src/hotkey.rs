//! The global mute hotkey: checking an accelerator (pure, tested) and registering it with the
//! OS through `tauri-plugin-global-shortcut`.
//!
//! The plugin runs registration on the main thread and blocks the caller until it is done, so
//! [`apply`] is only called from the main thread (app setup and synchronous commands).

use crate::service::Service;
use std::str::FromStr;
use std::sync::{Mutex, PoisonError};
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_global_shortcut::{
    Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutEvent, ShortcutState,
};

/// What the hotkey recorder should say when no modifier that protects typing was used.
#[cfg(target_os = "macos")]
const NEEDS_MODIFIER: &str =
    "Add ⌘ Command, ⌥ Option or ⌃ Control to the hotkey, so it never takes over a key you type.";
#[cfg(not(target_os = "macos"))]
const NEEDS_MODIFIER: &str =
    "Add Ctrl, Alt or the Windows key to the hotkey, so it never takes over a key you type.";

/// Function keys work on their own; anything else needs one of these.
const GUARD_MODIFIERS: Modifiers = Modifiers::CONTROL.union(Modifiers::ALT).union(Modifiers::SUPER);

fn is_function_key(code: Code) -> bool {
    use Code::*;
    matches!(
        code,
        F1 | F2
            | F3
            | F4
            | F5
            | F6
            | F7
            | F8
            | F9
            | F10
            | F11
            | F12
            | F13
            | F14
            | F15
            | F16
            | F17
            | F18
            | F19
            | F20
            | F21
            | F22
            | F23
            | F24
    )
}

/// Media and volume keys: the OS (and media apps) own them, and some need an extra event tap.
fn is_media_key(code: Code) -> bool {
    use Code::*;
    matches!(
        code,
        AudioVolumeDown
            | AudioVolumeUp
            | AudioVolumeMute
            | MediaPlay
            | MediaPause
            | MediaPlayPause
            | MediaStop
            | MediaTrackNext
            | MediaTrackPrevious
            | MediaFastForward
            | MediaRewind
    )
}

/// Checks an accelerator such as `CommandOrControl+Alt+Shift+M` and returns its shortcut, or
/// a message for the user: unparseable, a media key, or a plain key that would swallow typing.
pub fn parse(accelerator: &str) -> Result<Shortcut, String> {
    let accelerator = accelerator.trim();
    let shortcut = Shortcut::from_str(accelerator).map_err(|_| {
        format!(
            "“{}” is not a shortcut TakTak understands. Use modifiers and one key, like \
             CommandOrControl+Alt+Shift+M.",
            taktak_core::pack::printable(accelerator)
        )
    })?;
    if is_media_key(shortcut.key) {
        return Err("Media and volume keys cannot be the hotkey.".to_owned());
    }
    if !is_function_key(shortcut.key) && !shortcut.mods.intersects(GUARD_MODIFIERS) {
        return Err(NEEDS_MODIFIER.to_owned());
    }
    Ok(shortcut)
}

/// `muteHotkeyError` for a saved hotkey that [`apply`] rejected at startup with `error`.
pub fn startup_error(error: &str) -> String {
    format!("The saved mute hotkey does not work. {error}")
}

/// The shortcut registered for mute, if any.
#[derive(Default)]
pub struct Hotkey(Mutex<Option<Shortcut>>);

impl Hotkey {
    fn get(&self) -> Option<Shortcut> {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn set(&self, shortcut: Option<Shortcut>) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = shortcut;
    }
}

/// Makes `accelerator` the mute hotkey (`None`: no hotkey). Registers the new one before
/// letting go of the old, so a rejected accelerator leaves the old hotkey working. Main thread
/// only (see the module docs).
pub fn apply<R: Runtime>(app: &AppHandle<R>, accelerator: Option<&str>) -> Result<(), String> {
    let wanted = accelerator.map(parse).transpose()?;
    let state = app.state::<Hotkey>();
    let current = state.get();
    if wanted.map(|s| s.id()) == current.map(|s| s.id()) {
        return Ok(());
    }
    let shortcuts = app.global_shortcut();
    if let Some(new) = wanted {
        shortcuts.register(new).map_err(|e| {
            log::warn!("cannot register the mute hotkey: {e}");
            format!(
                "{} is already used by another app or by the system. Choose another hotkey.",
                taktak_core::pack::printable(accelerator.unwrap_or_default().trim())
            )
        })?;
    }
    if let Some(old) = current
        && let Err(e) = shortcuts.unregister(old)
    {
        log::warn!("cannot unregister the previous mute hotkey: {e}");
    }
    state.set(wanted);
    Ok(())
}

/// The plugin's handler: the mute hotkey was pressed (the only shortcut TakTak registers).
pub fn on_event<R: Runtime>(app: &AppHandle<R>, _shortcut: &Shortcut, event: ShortcutEvent) {
    if event.state != ShortcutState::Pressed {
        return;
    }
    if let Some(service) = app.try_state::<Service>() {
        let state = service.update(|s| s.muted = !s.muted);
        log::info!("mute hotkey: {}", if state.muted { "muted" } else { "unmuted" });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_hotkey_is_valid() {
        let s = parse(crate::state::DEFAULT_MUTE_HOTKEY).unwrap();
        assert_eq!(s.key, Code::KeyM);
        assert!(s.mods.contains(Modifiers::ALT | Modifiers::SHIFT));
        assert!(s.mods.intersects(Modifiers::SUPER | Modifiers::CONTROL));
        assert_eq!(parse("  Ctrl+Shift+F9 ").unwrap().key, Code::F9);
    }

    #[test]
    fn unparseable_accelerators_are_rejected_with_a_message() {
        for bad in ["", "Ctrl+", "Ctrl+Shift", "Ctrl+M+K", "Hyper+M", "Ctrl+Nope", "M+Ctrl"] {
            let err = parse(bad).unwrap_err();
            assert!(err.contains("not a shortcut"), "{bad:?}: {err}");
        }
        // Control characters are escaped before they reach the UI.
        assert!(parse("Ctrl+\u{1b}[2J").unwrap_err().contains("\\u{1b}"));
    }

    #[test]
    fn plain_keys_need_a_modifier_but_function_keys_do_not() {
        assert_eq!(parse("M").unwrap_err(), NEEDS_MODIFIER);
        assert_eq!(parse("Shift+M").unwrap_err(), NEEDS_MODIFIER);
        assert_eq!(parse("Shift+Space").unwrap_err(), NEEDS_MODIFIER);
        assert!(parse("Alt+M").is_ok());
        assert!(parse("Super+Space").is_ok());
        assert!(parse("F13").is_ok());
        assert!(parse("Shift+F24").is_ok());
    }

    #[test]
    fn a_saved_hotkey_that_fails_at_startup_says_why() {
        // A hand-edited settings.json: parse() rejects it, so it was never registered.
        let why = parse("Shift+M").unwrap_err();
        let message = startup_error(&why);
        assert!(message.starts_with("The saved mute hotkey does not work."), "{message}");
        assert!(message.ends_with(NEEDS_MODIFIER), "{message}");
    }

    #[test]
    fn media_keys_are_rejected() {
        assert!(parse("Ctrl+VolumeMute").unwrap_err().contains("Media"));
        assert!(parse("MediaPlayPause").unwrap_err().contains("Media"));
    }
}
