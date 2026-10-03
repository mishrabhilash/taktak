//! Platform-neutral key identities, named after the web `KeyboardEvent.code` values
//! (<https://www.w3.org/TR/uievents-code/>). Native scan/virtual codes are mapped to these in
//! the per-platform `input` modules; sound packs refer to keys by these names.

use std::fmt;
use std::str::FromStr;

macro_rules! keys {
    ($($variant:ident => $name:literal),+ $(,)?) => {
        /// A physical key position. `Debug` is redacted on purpose (privacy): use
        /// [`Key::code_name`] explicitly where a name is genuinely needed (pack lookup, tests).
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[repr(u8)]
        pub enum Key { $($variant),+ }

        impl Key {
            pub const ALL: &'static [Key] = &[$(Key::$variant),+];

            /// The `KeyboardEvent.code` name, e.g. `"KeyA"`, `"Space"`.
            pub const fn code_name(self) -> &'static str {
                match self { $(Key::$variant => $name),+ }
            }
        }

        impl FromStr for Key {
            type Err = UnknownKey;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $($name => Ok(Key::$variant),)+
                    _ => Err(UnknownKey(s.to_owned())),
                }
            }
        }
    };
}

keys! {
    KeyA => "KeyA", KeyB => "KeyB", KeyC => "KeyC", KeyD => "KeyD", KeyE => "KeyE",
    KeyF => "KeyF", KeyG => "KeyG", KeyH => "KeyH", KeyI => "KeyI", KeyJ => "KeyJ",
    KeyK => "KeyK", KeyL => "KeyL", KeyM => "KeyM", KeyN => "KeyN", KeyO => "KeyO",
    KeyP => "KeyP", KeyQ => "KeyQ", KeyR => "KeyR", KeyS => "KeyS", KeyT => "KeyT",
    KeyU => "KeyU", KeyV => "KeyV", KeyW => "KeyW", KeyX => "KeyX", KeyY => "KeyY",
    KeyZ => "KeyZ",
    Digit0 => "Digit0", Digit1 => "Digit1", Digit2 => "Digit2", Digit3 => "Digit3",
    Digit4 => "Digit4", Digit5 => "Digit5", Digit6 => "Digit6", Digit7 => "Digit7",
    Digit8 => "Digit8", Digit9 => "Digit9",
    Backquote => "Backquote", Minus => "Minus", Equal => "Equal",
    BracketLeft => "BracketLeft", BracketRight => "BracketRight", Backslash => "Backslash",
    Semicolon => "Semicolon", Quote => "Quote", Comma => "Comma", Period => "Period",
    Slash => "Slash", IntlBackslash => "IntlBackslash", IntlRo => "IntlRo", IntlYen => "IntlYen",
    Space => "Space", Enter => "Enter", Tab => "Tab", Backspace => "Backspace",
    Escape => "Escape", CapsLock => "CapsLock",
    ShiftLeft => "ShiftLeft", ShiftRight => "ShiftRight",
    ControlLeft => "ControlLeft", ControlRight => "ControlRight",
    AltLeft => "AltLeft", AltRight => "AltRight",
    MetaLeft => "MetaLeft", MetaRight => "MetaRight",
    ContextMenu => "ContextMenu", Fn => "Fn",
    F1 => "F1", F2 => "F2", F3 => "F3", F4 => "F4", F5 => "F5", F6 => "F6",
    F7 => "F7", F8 => "F8", F9 => "F9", F10 => "F10", F11 => "F11", F12 => "F12",
    F13 => "F13", F14 => "F14", F15 => "F15", F16 => "F16", F17 => "F17", F18 => "F18",
    F19 => "F19", F20 => "F20", F21 => "F21", F22 => "F22", F23 => "F23", F24 => "F24",
    ArrowUp => "ArrowUp", ArrowDown => "ArrowDown", ArrowLeft => "ArrowLeft",
    ArrowRight => "ArrowRight",
    Home => "Home", End => "End", PageUp => "PageUp", PageDown => "PageDown",
    Insert => "Insert", Delete => "Delete", Help => "Help",
    PrintScreen => "PrintScreen", ScrollLock => "ScrollLock", Pause => "Pause",
    NumLock => "NumLock",
    Numpad0 => "Numpad0", Numpad1 => "Numpad1", Numpad2 => "Numpad2", Numpad3 => "Numpad3",
    Numpad4 => "Numpad4", Numpad5 => "Numpad5", Numpad6 => "Numpad6", Numpad7 => "Numpad7",
    Numpad8 => "Numpad8", Numpad9 => "Numpad9",
    NumpadAdd => "NumpadAdd", NumpadSubtract => "NumpadSubtract",
    NumpadMultiply => "NumpadMultiply", NumpadDivide => "NumpadDivide",
    NumpadDecimal => "NumpadDecimal", NumpadEnter => "NumpadEnter",
    NumpadEqual => "NumpadEqual", NumpadComma => "NumpadComma",
    AudioVolumeUp => "AudioVolumeUp", AudioVolumeDown => "AudioVolumeDown",
    AudioVolumeMute => "AudioVolumeMute",
    Lang1 => "Lang1", Lang2 => "Lang2", KanaMode => "KanaMode",
    Convert => "Convert", NonConvert => "NonConvert",
}

/// Fallback groups used when a pack has no sound for an individual key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeyGroup {
    Alphanumeric,
    Space,
    Enter,
    Backspace,
    Modifier,
    Other,
}

impl KeyGroup {
    pub const fn name(self) -> &'static str {
        match self {
            KeyGroup::Alphanumeric => "alphanumeric",
            KeyGroup::Space => "space",
            KeyGroup::Enter => "enter",
            KeyGroup::Backspace => "backspace",
            KeyGroup::Modifier => "modifiers",
            KeyGroup::Other => "other",
        }
    }
}

impl Key {
    pub const COUNT: usize = Key::ALL.len();

    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn group(self) -> KeyGroup {
        use Key::*;
        match self {
            Space => KeyGroup::Space,
            Enter | NumpadEnter => KeyGroup::Enter,
            Backspace | Delete => KeyGroup::Backspace,
            ShiftLeft | ShiftRight | ControlLeft | ControlRight | AltLeft | AltRight | MetaLeft
            | MetaRight | CapsLock | Fn => KeyGroup::Modifier,
            KeyA | KeyB | KeyC | KeyD | KeyE | KeyF | KeyG | KeyH | KeyI | KeyJ | KeyK | KeyL
            | KeyM | KeyN | KeyO | KeyP | KeyQ | KeyR | KeyS | KeyT | KeyU | KeyV | KeyW | KeyX
            | KeyY | KeyZ | Digit0 | Digit1 | Digit2 | Digit3 | Digit4 | Digit5 | Digit6
            | Digit7 | Digit8 | Digit9 | Backquote | Minus | Equal | BracketLeft | BracketRight
            | Backslash | Semicolon | Quote | Comma | Period | Slash | IntlBackslash | IntlRo
            | IntlYen => KeyGroup::Alphanumeric,
            _ => KeyGroup::Other,
        }
    }
}

// `input::PressState` tracks held keys in a 256-bit set indexed by `Key::index`.
const _: () = assert!(Key::COUNT <= 256, "Key must fit in a u8-indexed bitset");

impl fmt::Debug for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Key(<redacted>)")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownKey(pub String);

impl fmt::Display for UnknownKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unknown key name {:?} (expected a KeyboardEvent.code name such as \"KeyA\")",
            self.0
        )
    }
}

impl std::error::Error for UnknownKey {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn names_round_trip_and_are_unique() {
        let mut seen = HashSet::new();
        for &k in Key::ALL {
            assert!(seen.insert(k.code_name()), "duplicate name {}", k.code_name());
            assert_eq!(k.code_name().parse::<Key>().unwrap(), k);
        }
    }

    #[test]
    fn index_matches_position() {
        for (i, &k) in Key::ALL.iter().enumerate() {
            assert_eq!(k.index(), i);
        }
    }

    #[test]
    fn unknown_name_is_an_error() {
        assert!("keya".parse::<Key>().is_err());
        assert!("A".parse::<Key>().is_err());
    }

    #[test]
    fn groups() {
        assert_eq!(Key::KeyQ.group(), KeyGroup::Alphanumeric);
        assert_eq!(Key::Slash.group(), KeyGroup::Alphanumeric);
        assert_eq!(Key::Space.group(), KeyGroup::Space);
        assert_eq!(Key::NumpadEnter.group(), KeyGroup::Enter);
        assert_eq!(Key::Delete.group(), KeyGroup::Backspace);
        assert_eq!(Key::MetaRight.group(), KeyGroup::Modifier);
        assert_eq!(Key::F5.group(), KeyGroup::Other);
    }

    #[test]
    fn debug_is_redacted() {
        assert_eq!(format!("{:?}", Key::KeyP), "Key(<redacted>)");
    }
}
