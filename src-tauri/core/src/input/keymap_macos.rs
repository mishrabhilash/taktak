//! macOS virtual key codes (`kVK_*` from HIToolbox/Events.h) → [`Key`].
//! Pure data, compiled on every platform so its tests run everywhere in CI.

use crate::key::Key;

pub const fn from_virtual_keycode(code: u16) -> Option<Key> {
    use Key::*;
    Some(match code {
        0x00 => KeyA,
        0x01 => KeyS,
        0x02 => KeyD,
        0x03 => KeyF,
        0x04 => KeyH,
        0x05 => KeyG,
        0x06 => KeyZ,
        0x07 => KeyX,
        0x08 => KeyC,
        0x09 => KeyV,
        0x0A => IntlBackslash, // kVK_ISO_Section
        0x0B => KeyB,
        0x0C => KeyQ,
        0x0D => KeyW,
        0x0E => KeyE,
        0x0F => KeyR,
        0x10 => KeyY,
        0x11 => KeyT,
        0x12 => Digit1,
        0x13 => Digit2,
        0x14 => Digit3,
        0x15 => Digit4,
        0x16 => Digit6,
        0x17 => Digit5,
        0x18 => Equal,
        0x19 => Digit9,
        0x1A => Digit7,
        0x1B => Minus,
        0x1C => Digit8,
        0x1D => Digit0,
        0x1E => BracketRight,
        0x1F => KeyO,
        0x20 => KeyU,
        0x21 => BracketLeft,
        0x22 => KeyI,
        0x23 => KeyP,
        0x24 => Enter,
        0x25 => KeyL,
        0x26 => KeyJ,
        0x27 => Quote,
        0x28 => KeyK,
        0x29 => Semicolon,
        0x2A => Backslash,
        0x2B => Comma,
        0x2C => Slash,
        0x2D => KeyN,
        0x2E => KeyM,
        0x2F => Period,
        0x30 => Tab,
        0x31 => Space,
        0x32 => Backquote,
        0x33 => Backspace, // kVK_Delete
        0x35 => Escape,
        0x36 => MetaRight,
        0x37 => MetaLeft,
        0x38 => ShiftLeft,
        0x39 => CapsLock,
        0x3A => AltLeft,
        0x3B => ControlLeft,
        0x3C => ShiftRight,
        0x3D => AltRight,
        0x3E => ControlRight,
        0x3F => Fn,
        0x40 => F17,
        0x41 => NumpadDecimal,
        0x43 => NumpadMultiply,
        0x45 => NumpadAdd,
        0x47 => NumLock, // kVK_ANSI_KeypadClear
        0x48 => AudioVolumeUp,
        0x49 => AudioVolumeDown,
        0x4A => AudioVolumeMute,
        0x4B => NumpadDivide,
        0x4C => NumpadEnter,
        0x4E => NumpadSubtract,
        0x4F => F18,
        0x50 => F19,
        0x51 => NumpadEqual,
        0x52 => Numpad0,
        0x53 => Numpad1,
        0x54 => Numpad2,
        0x55 => Numpad3,
        0x56 => Numpad4,
        0x57 => Numpad5,
        0x58 => Numpad6,
        0x59 => Numpad7,
        0x5A => F20,
        0x5B => Numpad8,
        0x5C => Numpad9,
        0x5D => IntlYen,
        0x5E => IntlRo,
        0x5F => NumpadComma,
        0x60 => F5,
        0x61 => F6,
        0x62 => F7,
        0x63 => F3,
        0x64 => F8,
        0x65 => F9,
        0x66 => Lang2, // kVK_JIS_Eisu
        0x67 => F11,
        0x68 => Lang1, // kVK_JIS_Kana
        0x69 => F13,
        0x6A => F16,
        0x6B => F14,
        0x6D => F10,
        0x6E => ContextMenu,
        0x6F => F12,
        0x71 => F15,
        0x72 => Help, // Insert position on PC keyboards
        0x73 => Home,
        0x74 => PageUp,
        0x75 => Delete, // kVK_ForwardDelete
        0x76 => F4,
        0x77 => End,
        0x78 => F2,
        0x79 => PageDown,
        0x7A => F1,
        0x7B => ArrowLeft,
        0x7C => ArrowRight,
        0x7D => ArrowDown,
        0x7E => ArrowUp,
        _ => return None,
    })
}

/// For `FlagsChanged` events: the device-dependent flag bit that says whether this modifier
/// is currently held (`NX_DEVICE*KEYMASK`, plus `kCGEventFlagMaskSecondaryFn` for Fn).
/// Caps Lock is absent: macOS reports its lock *state*, not the physical key, so it is
/// treated as press-only.
pub const fn modifier_held_mask(code: u16) -> Option<u64> {
    Some(match code {
        0x3B => 0x0000_0001, // left control
        0x38 => 0x0000_0002, // left shift
        0x3C => 0x0000_0004, // right shift
        0x37 => 0x0000_0008, // left command
        0x36 => 0x0000_0010, // right command
        0x3A => 0x0000_0020, // left option
        0x3D => 0x0000_0040, // right option
        0x3E => 0x0000_2000, // right control
        0x3F => 0x0080_0000, // fn
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::KeyGroup;
    use std::collections::HashMap;

    #[test]
    fn known_codes() {
        assert_eq!(from_virtual_keycode(0x00), Some(Key::KeyA));
        assert_eq!(from_virtual_keycode(0x31), Some(Key::Space));
        assert_eq!(from_virtual_keycode(0x24), Some(Key::Enter));
        assert_eq!(from_virtual_keycode(0x33), Some(Key::Backspace));
        assert_eq!(from_virtual_keycode(0x75), Some(Key::Delete));
        assert_eq!(from_virtual_keycode(0x37), Some(Key::MetaLeft));
        assert_eq!(from_virtual_keycode(0x7E), Some(Key::ArrowUp));
        assert_eq!(from_virtual_keycode(0x34), None);
        assert_eq!(from_virtual_keycode(0xFFFF), None);
    }

    #[test]
    fn mapping_is_injective() {
        let mut seen: HashMap<Key, u16> = HashMap::new();
        for code in 0..=0x7Fu16 {
            if let Some(k) = from_virtual_keycode(code)
                && let Some(prev) = seen.insert(k, code)
            {
                panic!("{} mapped from both {prev:#x} and {code:#x}", k.code_name());
            }
        }
        // Every letter and digit is reachable.
        let alnum = seen.keys().filter(|k| k.group() == KeyGroup::Alphanumeric).count();
        assert!(alnum >= 26 + 10 + 11, "only {alnum} alphanumeric keys mapped");
    }

    #[test]
    fn every_modifier_mask_belongs_to_a_modifier_key() {
        for code in 0..=0x7Fu16 {
            if modifier_held_mask(code).is_some() {
                assert_eq!(from_virtual_keycode(code).unwrap().group(), KeyGroup::Modifier);
            }
        }
    }
}
