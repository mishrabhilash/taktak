//! Windows scan codes (PC set 1, as reported by `WH_KEYBOARD_LL` in `KBDLLHOOKSTRUCT`) → [`Key`].
//! Pure data, compiled on every platform so its tests run everywhere in CI.
//!
//! Scan codes name physical positions, so the result does not depend on the keyboard layout.
//! Virtual-key codes (`vkCode`) do depend on it (the key labelled Q on a US layout is `VK_A`
//! on a French one) and are never used, nor is `ToUnicode`/`ToUnicodeEx`.

use super::KeyAction;
use crate::key::Key;

/// `KBDLLHOOKSTRUCT.flags`: the key is an extended key (E0 prefix).
pub const LLKHF_EXTENDED: u32 = 0x01;
/// `KBDLLHOOKSTRUCT.flags`: the event was injected (`SendInput`, `keybd_event`).
pub const LLKHF_INJECTED: u32 = 0x10;
pub const WM_KEYDOWN: u32 = 0x0100;
pub const WM_KEYUP: u32 = 0x0101;
pub const WM_SYSKEYDOWN: u32 = 0x0104;
pub const WM_SYSKEYUP: u32 = 0x0105;

/// Whether injected events (`LLKHF_INJECTED`) make a sound. They come from on-screen keyboards,
/// remote-desktop and automation tools (AutoHotkey, macro software) and from apps that type
/// for the user; ignoring them keeps TakTak to keys the user physically pressed, and can never
/// echo back the keystrokes of a tool that synthesizes input. The cost: keys typed through an
/// on-screen keyboard or remapped by a tool that swallows the physical key are silent.
pub const PLAY_INJECTED: bool = false;

/// Maps a set-1 scan code to a key. `code` is the 8-bit scan code, with `0xE000` added when the
/// key is extended (`LLKHF_EXTENDED`), e.g. `0x1C` is Enter and `0xE01C` is Numpad Enter.
pub const fn from_scancode(code: u16) -> Option<Key> {
    use Key::*;
    Some(match code {
        0x01 => Escape,
        0x02 => Digit1,
        0x03 => Digit2,
        0x04 => Digit3,
        0x05 => Digit4,
        0x06 => Digit5,
        0x07 => Digit6,
        0x08 => Digit7,
        0x09 => Digit8,
        0x0A => Digit9,
        0x0B => Digit0,
        0x0C => Minus,
        0x0D => Equal,
        0x0E => Backspace,
        0x0F => Tab,
        0x10 => KeyQ,
        0x11 => KeyW,
        0x12 => KeyE,
        0x13 => KeyR,
        0x14 => KeyT,
        0x15 => KeyY,
        0x16 => KeyU,
        0x17 => KeyI,
        0x18 => KeyO,
        0x19 => KeyP,
        0x1A => BracketLeft,
        0x1B => BracketRight,
        0x1C => Enter,
        0x1D => ControlLeft,
        0x1E => KeyA,
        0x1F => KeyS,
        0x20 => KeyD,
        0x21 => KeyF,
        0x22 => KeyG,
        0x23 => KeyH,
        0x24 => KeyJ,
        0x25 => KeyK,
        0x26 => KeyL,
        0x27 => Semicolon,
        0x28 => Quote,
        0x29 => Backquote,
        0x2A => ShiftLeft,
        0x2B => Backslash,
        0x2C => KeyZ,
        0x2D => KeyX,
        0x2E => KeyC,
        0x2F => KeyV,
        0x30 => KeyB,
        0x31 => KeyN,
        0x32 => KeyM,
        0x33 => Comma,
        0x34 => Period,
        0x35 => Slash,
        0x36 => ShiftRight,
        0x37 => NumpadMultiply,
        0x38 => AltLeft,
        0x39 => Space,
        0x3A => CapsLock,
        0x3B => F1,
        0x3C => F2,
        0x3D => F3,
        0x3E => F4,
        0x3F => F5,
        0x40 => F6,
        0x41 => F7,
        0x42 => F8,
        0x43 => F9,
        0x44 => F10,
        // The hook (like WM_KEYDOWN) reports Pause as 0x45 without the extended flag and
        // Num Lock as 0x45 with it, the opposite of the raw scan code sequences (E1 1D 45 / 45).
        0x45 => Pause,
        0x46 => ScrollLock,
        0x47 => Numpad7,
        0x48 => Numpad8,
        0x49 => Numpad9,
        0x4A => NumpadSubtract,
        0x4B => Numpad4,
        0x4C => Numpad5,
        0x4D => Numpad6,
        0x4E => NumpadAdd,
        0x4F => Numpad1,
        0x50 => Numpad2,
        0x51 => Numpad3,
        0x52 => Numpad0,
        0x53 => NumpadDecimal,
        0x54 => PrintScreen, // alias: Alt+Print Screen (SysRq)
        0x56 => IntlBackslash,
        0x57 => F11,
        0x58 => F12,
        0x59 => NumpadEqual,
        0x64 => F13,
        0x65 => F14,
        0x66 => F15,
        0x67 => F16,
        0x68 => F17,
        0x69 => F18,
        0x6A => F19,
        0x6B => F20,
        0x6C => F21,
        0x6D => F22,
        0x6E => F23,
        0x70 => KanaMode,
        0x71 => Lang2,
        0x72 => Lang1,
        0x73 => IntlRo,
        0x76 => F24,
        0x79 => Convert,
        0x7B => NonConvert,
        0x7D => IntlYen,
        0x7E => NumpadComma,
        0xF1 => Lang2, // alias: Hanja on Korean keyboards
        0xF2 => Lang1, // alias: Hangul on Korean keyboards
        0xE01C => NumpadEnter,
        0xE01D => ControlRight,
        0xE020 => AudioVolumeMute,
        0xE02E => AudioVolumeDown,
        0xE030 => AudioVolumeUp,
        0xE035 => NumpadDivide,
        // alias: the low-level hook reports Right Shift with LLKHF_EXTENDED set.
        0xE036 => ShiftRight,
        0xE037 => PrintScreen,
        0xE038 => AltRight,
        0xE03B => Help,
        0xE045 => NumLock,
        0xE046 => Pause, // alias: Ctrl+Pause (Break)
        0xE047 => Home,
        0xE048 => ArrowUp,
        0xE049 => PageUp,
        0xE04B => ArrowLeft,
        0xE04D => ArrowRight,
        0xE04F => End,
        0xE050 => ArrowDown,
        0xE051 => PageDown,
        0xE052 => Insert,
        0xE053 => Delete,
        0xE05B => MetaLeft,
        0xE05C => MetaRight,
        0xE05D => ContextMenu,
        // 0xE02A / 0xE0AA (and 0xE036 / 0xE0B6 in the raw stream) are the "fake shift"
        // prefixes keyboards send around navigation keys; 0xE02A maps to nothing on purpose.
        _ => return None,
    })
}

/// Codes above that are a second name for a key that already has one.
pub const ALIASES: &[u16] = &[0x54, 0xF1, 0xF2, 0xE036, 0xE046];

/// Maps a `KBDLLHOOKSTRUCT` (`scanCode`, `flags`) and the hook's message (`wParam`) to a key
/// transition, or `None` for events that must make no sound:
/// - injected events, unless [`PLAY_INJECTED`];
/// - scan codes above 0xFF: the system marks the events it synthesizes with 0x200
///   (`SCANCODE_SIMULATED`): the left Ctrl that AltGr generates (0x21D) and the shift up/downs
///   it inserts around numpad keys when Num Lock is on (0x22A, 0x236). They are not presses;
///   AltGr itself still arrives as Right Alt (0xE038);
/// - scan code 0 (`VK_PACKET` text injection, some virtual keyboards) and unmapped codes.
pub const fn from_hook(scan_code: u32, flags: u32, message: u32) -> Option<(Key, KeyAction)> {
    if !PLAY_INJECTED && flags & LLKHF_INJECTED != 0 {
        return None;
    }
    if scan_code > 0xFF {
        return None;
    }
    let action = match message {
        WM_KEYDOWN | WM_SYSKEYDOWN => KeyAction::Down,
        WM_KEYUP | WM_SYSKEYUP => KeyAction::Up,
        _ => return None,
    };
    let extended = if flags & LLKHF_EXTENDED != 0 { 0xE000 } else { 0 };
    match from_scancode(scan_code as u16 | extended) {
        Some(key) => Some((key, action)),
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::KeyGroup;
    use std::collections::HashMap;

    fn all_codes() -> impl Iterator<Item = u16> {
        (0..=0xFFu16).chain(0xE000..=0xE0FF)
    }

    #[test]
    fn known_codes() {
        assert_eq!(from_scancode(0x1E), Some(Key::KeyA));
        assert_eq!(from_scancode(0x10), Some(Key::KeyQ));
        assert_eq!(from_scancode(0x39), Some(Key::Space));
        assert_eq!(from_scancode(0x1C), Some(Key::Enter));
        assert_eq!(from_scancode(0xE01C), Some(Key::NumpadEnter));
        assert_eq!(from_scancode(0x0E), Some(Key::Backspace));
        assert_eq!(from_scancode(0xE053), Some(Key::Delete));
        assert_eq!(from_scancode(0x53), Some(Key::NumpadDecimal));
        assert_eq!(from_scancode(0x47), Some(Key::Numpad7));
        assert_eq!(from_scancode(0xE047), Some(Key::Home));
        assert_eq!(from_scancode(0x45), Some(Key::Pause));
        assert_eq!(from_scancode(0xE045), Some(Key::NumLock));
        assert_eq!(from_scancode(0xE05B), Some(Key::MetaLeft));
        assert_eq!(from_scancode(0xE02A), None);
        assert_eq!(from_scancode(0x00), None);
        assert_eq!(from_scancode(0xFFFF), None);
    }

    #[test]
    fn mapping_is_injective_apart_from_aliases() {
        let mut seen: HashMap<Key, u16> = HashMap::new();
        for code in all_codes().filter(|c| !ALIASES.contains(c)) {
            if let Some(k) = from_scancode(code)
                && let Some(prev) = seen.insert(k, code)
            {
                panic!("{} mapped from both {prev:#x} and {code:#x}", k.code_name());
            }
        }
        for &alias in ALIASES {
            let key = from_scancode(alias).expect("alias maps to a key");
            assert!(seen.contains_key(&key), "alias {alias:#x} has no primary code");
        }
    }

    #[test]
    fn letters_digits_and_modifiers_are_complete() {
        let mapped: Vec<Key> = all_codes().filter_map(from_scancode).collect();
        for &k in Key::ALL {
            let name = k.code_name();
            let letter_or_digit = (name.starts_with("Key") && name.len() == 4)
                || (name.starts_with("Digit") && name.len() == 6);
            if letter_or_digit {
                assert!(mapped.contains(&k), "{name} is not mapped");
            }
        }
        for k in [
            Key::ShiftLeft,
            Key::ShiftRight,
            Key::ControlLeft,
            Key::ControlRight,
            Key::AltLeft,
            Key::AltRight,
            Key::MetaLeft,
            Key::MetaRight,
            Key::CapsLock,
        ] {
            assert!(mapped.contains(&k), "{} is not mapped", k.code_name());
            assert_eq!(k.group(), KeyGroup::Modifier);
        }
        let alnum = mapped.iter().filter(|k| k.group() == KeyGroup::Alphanumeric).count();
        assert!(alnum >= 26 + 10 + 11, "only {alnum} alphanumeric keys mapped");
    }

    #[test]
    fn modifier_scancodes_map_to_the_modifier_group() {
        for code in [0x1D, 0xE01D, 0x2A, 0x36, 0xE036, 0x38, 0xE038, 0xE05B, 0xE05C, 0x3A] {
            assert_eq!(from_scancode(code).unwrap().group(), KeyGroup::Modifier, "{code:#x}");
        }
    }

    #[test]
    fn hook_events() {
        use KeyAction::{Down, Up};
        assert_eq!(from_hook(0x1E, 0, WM_KEYDOWN), Some((Key::KeyA, Down)));
        assert_eq!(from_hook(0x1E, 0x80, WM_KEYUP), Some((Key::KeyA, Up)));
        // Alt held: WM_SYS* messages.
        assert_eq!(from_hook(0x3E, 0x20, WM_SYSKEYDOWN), Some((Key::F4, Down)));
        assert_eq!(from_hook(0x38, 0x80, WM_SYSKEYUP), Some((Key::AltLeft, Up)));
        assert_eq!(from_hook(0x1C, LLKHF_EXTENDED, WM_KEYDOWN), Some((Key::NumpadEnter, Down)));
        assert_eq!(from_hook(0x36, LLKHF_EXTENDED, WM_KEYDOWN), Some((Key::ShiftRight, Down)));
        assert_eq!(from_hook(0x45, LLKHF_EXTENDED, WM_KEYDOWN), Some((Key::NumLock, Down)));
        assert_eq!(from_hook(0x45, 0, WM_KEYDOWN), Some((Key::Pause, Down)));
        // AltGr's synthesized left Ctrl and the system's fake shifts are not presses.
        assert_eq!(from_hook(0x21D, 0, WM_KEYDOWN), None);
        assert_eq!(from_hook(0x22A, 0x80, WM_KEYUP), None);
        assert_eq!(from_hook(0x236, LLKHF_EXTENDED, WM_KEYDOWN), None);
        assert_eq!(
            from_hook(0x38, LLKHF_EXTENDED | 0x20, WM_SYSKEYDOWN),
            Some((Key::AltRight, Down))
        );
        // Injected and text (VK_PACKET, scan code 0) events are ignored.
        assert_eq!(from_hook(0x1E, LLKHF_INJECTED, WM_KEYDOWN), None);
        assert_eq!(from_hook(0, 0, WM_KEYDOWN), None);
        assert_eq!(from_hook(0x1E, 0, 0x0102), None); // WM_CHAR never reaches a LL hook
    }
}
