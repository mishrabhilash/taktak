//! Linux key codes (`KEY_*` from `linux/input-event-codes.h`, what evdev reports) → [`Key`].
//! X11 key codes on any current X server (evdev or libinput driver) are these plus 8.
//! Pure data, compiled on every platform so its tests run everywhere in CI.
//!
//! Key codes name physical positions; no keysym, XKB or layout lookups are ever made, so no
//! characters are produced.

use crate::key::Key;

/// The offset between evdev key codes and X11 key codes (`detail` of XInput2 key events).
pub const X11_KEYCODE_OFFSET: u32 = 8;

pub const fn from_evdev_code(code: u16) -> Option<Key> {
    use Key::*;
    Some(match code {
        1 => Escape,
        2 => Digit1,
        3 => Digit2,
        4 => Digit3,
        5 => Digit4,
        6 => Digit5,
        7 => Digit6,
        8 => Digit7,
        9 => Digit8,
        10 => Digit9,
        11 => Digit0,
        12 => Minus,
        13 => Equal,
        14 => Backspace,
        15 => Tab,
        16 => KeyQ,
        17 => KeyW,
        18 => KeyE,
        19 => KeyR,
        20 => KeyT,
        21 => KeyY,
        22 => KeyU,
        23 => KeyI,
        24 => KeyO,
        25 => KeyP,
        26 => BracketLeft,  // KEY_LEFTBRACE
        27 => BracketRight, // KEY_RIGHTBRACE
        28 => Enter,
        29 => ControlLeft,
        30 => KeyA,
        31 => KeyS,
        32 => KeyD,
        33 => KeyF,
        34 => KeyG,
        35 => KeyH,
        36 => KeyJ,
        37 => KeyK,
        38 => KeyL,
        39 => Semicolon,
        40 => Quote,     // KEY_APOSTROPHE
        41 => Backquote, // KEY_GRAVE
        42 => ShiftLeft,
        43 => Backslash,
        44 => KeyZ,
        45 => KeyX,
        46 => KeyC,
        47 => KeyV,
        48 => KeyB,
        49 => KeyN,
        50 => KeyM,
        51 => Comma,
        52 => Period, // KEY_DOT
        53 => Slash,
        54 => ShiftRight,
        55 => NumpadMultiply, // KEY_KPASTERISK
        56 => AltLeft,
        57 => Space,
        58 => CapsLock,
        59 => F1,
        60 => F2,
        61 => F3,
        62 => F4,
        63 => F5,
        64 => F6,
        65 => F7,
        66 => F8,
        67 => F9,
        68 => F10,
        69 => NumLock,
        70 => ScrollLock,
        71 => Numpad7,
        72 => Numpad8,
        73 => Numpad9,
        74 => NumpadSubtract,
        75 => Numpad4,
        76 => Numpad5,
        77 => Numpad6,
        78 => NumpadAdd,
        79 => Numpad1,
        80 => Numpad2,
        81 => Numpad3,
        82 => Numpad0,
        83 => NumpadDecimal,
        86 => IntlBackslash, // KEY_102ND
        87 => F11,
        88 => F12,
        89 => IntlRo,      // KEY_RO
        92 => Convert,     // KEY_HENKAN
        93 => KanaMode,    // KEY_KATAKANAHIRAGANA
        94 => NonConvert,  // KEY_MUHENKAN
        96 => NumpadEnter, // KEY_KPENTER
        97 => ControlRight,
        98 => NumpadDivide,
        99 => PrintScreen, // KEY_SYSRQ
        100 => AltRight,
        102 => Home,
        103 => ArrowUp,
        104 => PageUp,
        105 => ArrowLeft,
        106 => ArrowRight,
        107 => End,
        108 => ArrowDown,
        109 => PageDown,
        110 => Insert,
        111 => Delete,
        113 => AudioVolumeMute,
        114 => AudioVolumeDown,
        115 => AudioVolumeUp,
        117 => NumpadEqual,
        119 => Pause,
        121 => NumpadComma,
        122 => Lang1, // KEY_HANGEUL
        123 => Lang2, // KEY_HANJA
        124 => IntlYen,
        125 => MetaLeft,
        126 => MetaRight,
        127 => ContextMenu, // KEY_COMPOSE
        138 => Help,
        183 => F13,
        184 => F14,
        185 => F15,
        186 => F16,
        187 => F17,
        188 => F18,
        189 => F19,
        190 => F20,
        191 => F21,
        192 => F22,
        193 => F23,
        194 => F24,
        0x1D0 => Fn, // KEY_FN: only some laptops report it; X11 key codes cannot reach it
        _ => return None,
    })
}

/// An X11 key code (XInput2 `detail`) → key: the evdev code plus [`X11_KEYCODE_OFFSET`].
pub const fn from_x11_keycode(keycode: u32) -> Option<Key> {
    match keycode.checked_sub(X11_KEYCODE_OFFSET) {
        Some(code) if code <= u16::MAX as u32 => from_evdev_code(code as u16),
        _ => None,
    }
}

/// The highest key code [`from_evdev_code`] maps, for sizing scans.
pub const MAX_MAPPED_CODE: u16 = 0x1D0;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::KeyGroup;
    use std::collections::HashMap;

    #[test]
    fn known_codes() {
        assert_eq!(from_evdev_code(30), Some(Key::KeyA));
        assert_eq!(from_evdev_code(16), Some(Key::KeyQ));
        assert_eq!(from_evdev_code(57), Some(Key::Space));
        assert_eq!(from_evdev_code(28), Some(Key::Enter));
        assert_eq!(from_evdev_code(96), Some(Key::NumpadEnter));
        assert_eq!(from_evdev_code(14), Some(Key::Backspace));
        assert_eq!(from_evdev_code(111), Some(Key::Delete));
        assert_eq!(from_evdev_code(125), Some(Key::MetaLeft));
        assert_eq!(from_evdev_code(0), None);
        assert_eq!(from_evdev_code(0x110), None); // BTN_LEFT
        assert_eq!(from_x11_keycode(38), Some(Key::KeyA));
        assert_eq!(from_x11_keycode(65), Some(Key::Space));
        assert_eq!(from_x11_keycode(7), None);
        assert_eq!(from_x11_keycode(u32::MAX), None);
    }

    #[test]
    fn mapping_is_injective() {
        let mut seen: HashMap<Key, u16> = HashMap::new();
        for code in 0..=0x2FFu16 {
            if let Some(k) = from_evdev_code(code) {
                assert!(code <= MAX_MAPPED_CODE);
                if let Some(prev) = seen.insert(k, code) {
                    panic!("{} mapped from both {prev} and {code}", k.code_name());
                }
            }
        }
        let alnum = seen.keys().filter(|k| k.group() == KeyGroup::Alphanumeric).count();
        assert!(alnum >= 26 + 10 + 11, "only {alnum} alphanumeric keys mapped");
    }

    #[test]
    fn letters_and_digits_are_complete() {
        for &k in Key::ALL {
            let name = k.code_name();
            let letter_or_digit = (name.starts_with("Key") && name.len() == 4)
                || (name.starts_with("Digit") && name.len() == 6);
            if letter_or_digit {
                assert!((0..=0xFFu16).any(|c| from_evdev_code(c) == Some(k)), "{name}");
            }
        }
    }

    #[test]
    fn modifier_codes_map_to_the_modifier_group() {
        // KEY_LEFTCTRL, RIGHTCTRL, LEFTSHIFT, RIGHTSHIFT, LEFTALT, RIGHTALT, LEFTMETA,
        // RIGHTMETA, CAPSLOCK, FN.
        for code in [29, 97, 42, 54, 56, 100, 125, 126, 58, 0x1D0] {
            assert_eq!(from_evdev_code(code).unwrap().group(), KeyGroup::Modifier, "{code}");
        }
    }
}
