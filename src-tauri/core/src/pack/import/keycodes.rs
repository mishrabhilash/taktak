//! Mechvibes key codes: libuiohook (iohook 0.9.3) virtual key codes, written as decimal
//! strings in a Mechvibes `config.json`, mapped to W3C `KeyboardEvent.code` names.
//!
//! The table is vendored from the Milestone 5 research (`mechvibes-keycodes.json`, 168 pairs),
//! generated from the iohook 0.9.3 header and platform tables, Mechvibes' `keycodes.js` and
//! Chromium's `dom_code_data.inc`. Codes 60999–61011 are Windows-only aliases of the
//! dedicated navigation and arrow keys. Some names (media, browser and Sun keys, `Lang4`) are
//! not TakTak keys; they are kept so an import can say "not supported by TakTak" instead of
//! "unknown code".
//!
//! Mechvibes' own per-OS remaps (macOS F13 = ArrowUp and so on) encode position guesses and
//! bugs, so they are deliberately not replicated: codes map by libuiohook meaning.

use crate::key::Key;

/// `(Mechvibes code, KeyboardEvent.code)`, sorted by code.
#[rustfmt::skip]
pub const MECHVIBES_KEYCODES: [(u32, &str); 168] = [
    (1, "Escape"), (2, "Digit1"), (3, "Digit2"), (4, "Digit3"), (5, "Digit4"), (6, "Digit5"),
    (7, "Digit6"), (8, "Digit7"), (9, "Digit8"), (10, "Digit9"), (11, "Digit0"), (12, "Minus"),
    (13, "Equal"), (14, "Backspace"), (15, "Tab"), (16, "KeyQ"), (17, "KeyW"), (18, "KeyE"),
    (19, "KeyR"), (20, "KeyT"), (21, "KeyY"), (22, "KeyU"), (23, "KeyI"), (24, "KeyO"),
    (25, "KeyP"), (26, "BracketLeft"), (27, "BracketRight"), (28, "Enter"), (29, "ControlLeft"),
    (30, "KeyA"), (31, "KeyS"), (32, "KeyD"), (33, "KeyF"), (34, "KeyG"), (35, "KeyH"),
    (36, "KeyJ"), (37, "KeyK"), (38, "KeyL"), (39, "Semicolon"), (40, "Quote"), (41, "Backquote"),
    (42, "ShiftLeft"), (43, "Backslash"), (44, "KeyZ"), (45, "KeyX"), (46, "KeyC"), (47, "KeyV"),
    (48, "KeyB"), (49, "KeyN"), (50, "KeyM"), (51, "Comma"), (52, "Period"), (53, "Slash"),
    (54, "ShiftRight"), (55, "NumpadMultiply"), (56, "AltLeft"), (57, "Space"), (58, "CapsLock"),
    (59, "F1"), (60, "F2"), (61, "F3"), (62, "F4"), (63, "F5"), (64, "F6"), (65, "F7"), (66, "F8"),
    (67, "F9"), (68, "F10"), (69, "NumLock"), (70, "ScrollLock"), (71, "Numpad7"), (72, "Numpad8"),
    (73, "Numpad9"), (74, "NumpadSubtract"), (75, "Numpad4"), (76, "Numpad5"), (77, "Numpad6"),
    (78, "NumpadAdd"), (79, "Numpad1"), (80, "Numpad2"), (81, "Numpad3"), (82, "Numpad0"),
    (83, "NumpadDecimal"), (87, "F11"), (88, "F12"), (91, "F13"), (92, "F14"), (93, "F15"),
    (99, "F16"), (100, "F17"), (101, "F18"), (102, "F19"), (103, "F20"), (104, "F21"),
    (105, "F22"), (106, "F23"), (107, "F24"), (112, "KanaMode"), (115, "IntlRo"), (119, "Lang4"),
    (121, "Convert"), (123, "NonConvert"), (125, "IntlYen"), (126, "NumpadComma"),
    (3597, "NumpadEqual"), (3612, "NumpadEnter"), (3613, "ControlRight"), (3637, "NumpadDivide"),
    (3639, "PrintScreen"), (3640, "AltRight"), (3653, "Pause"), (3655, "Home"), (3657, "PageUp"),
    (3663, "End"), (3665, "PageDown"), (3666, "Insert"), (3667, "Delete"), (3675, "MetaLeft"),
    (3676, "MetaRight"), (3677, "ContextMenu"), (57360, "MediaTrackPrevious"),
    (57369, "MediaTrackNext"), (57376, "AudioVolumeMute"), (57377, "LaunchApp2"),
    (57378, "MediaPlayPause"), (57380, "MediaStop"), (57388, "Eject"), (57390, "AudioVolumeDown"),
    (57392, "AudioVolumeUp"), (57394, "BrowserHome"), (57416, "ArrowUp"), (57419, "ArrowLeft"),
    (57420, "Numpad5"), (57421, "ArrowRight"), (57424, "ArrowDown"), (57438, "Power"),
    (57439, "Sleep"), (57443, "WakeUp"), (57445, "BrowserSearch"), (57446, "BrowserFavorites"),
    (57447, "BrowserRefresh"), (57448, "BrowserStop"), (57449, "BrowserForward"),
    (57450, "BrowserBack"), (57452, "LaunchMail"), (57453, "MediaSelect"), (60999, "Home"),
    (61000, "ArrowUp"), (61001, "PageUp"), (61003, "ArrowLeft"), (61004, "Numpad5"),
    (61005, "ArrowRight"), (61007, "End"), (61008, "ArrowDown"), (61009, "PageDown"),
    (61010, "Insert"), (61011, "Delete"), (65396, "Open"), (65397, "Help"), (65398, "Props"),
    (65399, "Select"), (65400, "BrowserStop"), (65401, "Again"), (65402, "Undo"), (65403, "Cut"),
    (65404, "Copy"), (65405, "Paste"), (65406, "Find"),
];

/// Linux evdev `KEY_*` codes as some Linux-oriented re-packs use them (above 83 they differ
/// from libuiohook). Applied only to keys that no libuiohook code in the pack defines.
#[rustfmt::skip]
pub const EVDEV_KEYCODES: [(u32, &str); 33] = [
    (85, "Backquote"), (86, "IntlBackslash"), (89, "IntlRo"), (90, "Lang3"), (94, "NonConvert"),
    (95, "NumpadComma"), (96, "NumpadEnter"), (97, "ControlRight"), (98, "NumpadDivide"),
    (99, "PrintScreen"), (100, "AltRight"), (102, "Home"), (103, "ArrowUp"), (104, "PageUp"),
    (105, "ArrowLeft"), (106, "ArrowRight"), (107, "End"), (108, "ArrowDown"),
    (109, "PageDown"), (110, "Insert"), (111, "Delete"), (113, "AudioVolumeMute"),
    (114, "AudioVolumeDown"), (115, "AudioVolumeUp"), (117, "NumpadEqual"), (119, "Pause"),
    (121, "NumpadComma"), (122, "Lang1"), (123, "Lang2"), (124, "IntlYen"), (125, "MetaLeft"),
    (126, "MetaRight"), (127, "ContextMenu"),
];

/// The `KeyboardEvent.code` name of a Mechvibes code, if the code is known.
pub fn code_name(code: u32) -> Option<&'static str> {
    MECHVIBES_KEYCODES
        .binary_search_by_key(&code, |&(c, _)| c)
        .ok()
        .map(|i| MECHVIBES_KEYCODES[i].1)
}

/// The evdev meaning of `code`, if the evdev table has one.
pub fn evdev_name(code: u32) -> Option<&'static str> {
    EVDEV_KEYCODES.iter().find(|&&(c, _)| c == code).map(|&(_, n)| n)
}

/// Codes 60999–61011: what iohook reports on Windows for the dedicated (not numpad) navigation
/// and arrow keys. Mechvibes overwrites them with the standard code's sound at load, so the
/// standard code wins and an alias only fills a gap.
pub fn is_windows_alias(code: u32) -> bool {
    (60_999..=61_011).contains(&code)
}

/// Codes that name the same key as a primary code: the Windows aliases, and `VC_CLEAR`
/// (57420, `Numpad5` with NumLock off). Like the aliases, they only fill a gap.
pub fn is_secondary(code: u32) -> bool {
    is_windows_alias(code) || code == 57_420
}

/// Evdev codes that libuiohook never uses (keypad Enter, right Control, keypad Divide, the
/// arrow and navigation block): a pack with one of them was numbered for Linux, so its
/// ambiguous codes 99–127 are read as evdev too.
pub fn is_strong_evdev_signal(code: u32) -> bool {
    matches!(code, 96 | 97 | 98 | 108 | 109 | 110 | 111)
}

/// Resolves a W3C name to a TakTak key, or explains why it cannot be used.
pub fn key_for_name(name: &str) -> Result<Key, String> {
    name.parse::<Key>().map_err(|_| format!("{name} is not a key TakTak supports"))
}

/// Resolves a libuiohook code to a TakTak key, or explains why it cannot be used.
pub fn lookup(code: u32) -> Result<Key, String> {
    match code_name(code) {
        Some(name) => key_for_name(name),
        None => Err("unknown Mechvibes key code".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// The research's `mechvibes-keycodes.json`, verbatim, as a cross-check of the table.
    const RESEARCH_JSON: &str = r#"[[1,"Escape"],[2,"Digit1"],[3,"Digit2"],[4,"Digit3"],[5,"Digit4"],[6,"Digit5"],[7,"Digit6"],[8,"Digit7"],[9,"Digit8"],[10,"Digit9"],[11,"Digit0"],[12,"Minus"],[13,"Equal"],[14,"Backspace"],[15,"Tab"],[16,"KeyQ"],[17,"KeyW"],[18,"KeyE"],[19,"KeyR"],[20,"KeyT"],[21,"KeyY"],[22,"KeyU"],[23,"KeyI"],[24,"KeyO"],[25,"KeyP"],[26,"BracketLeft"],[27,"BracketRight"],[28,"Enter"],[29,"ControlLeft"],[30,"KeyA"],[31,"KeyS"],[32,"KeyD"],[33,"KeyF"],[34,"KeyG"],[35,"KeyH"],[36,"KeyJ"],[37,"KeyK"],[38,"KeyL"],[39,"Semicolon"],[40,"Quote"],[41,"Backquote"],[42,"ShiftLeft"],[43,"Backslash"],[44,"KeyZ"],[45,"KeyX"],[46,"KeyC"],[47,"KeyV"],[48,"KeyB"],[49,"KeyN"],[50,"KeyM"],[51,"Comma"],[52,"Period"],[53,"Slash"],[54,"ShiftRight"],[55,"NumpadMultiply"],[56,"AltLeft"],[57,"Space"],[58,"CapsLock"],[59,"F1"],[60,"F2"],[61,"F3"],[62,"F4"],[63,"F5"],[64,"F6"],[65,"F7"],[66,"F8"],[67,"F9"],[68,"F10"],[69,"NumLock"],[70,"ScrollLock"],[71,"Numpad7"],[72,"Numpad8"],[73,"Numpad9"],[74,"NumpadSubtract"],[75,"Numpad4"],[76,"Numpad5"],[77,"Numpad6"],[78,"NumpadAdd"],[79,"Numpad1"],[80,"Numpad2"],[81,"Numpad3"],[82,"Numpad0"],[83,"NumpadDecimal"],[87,"F11"],[88,"F12"],[91,"F13"],[92,"F14"],[93,"F15"],[99,"F16"],[100,"F17"],[101,"F18"],[102,"F19"],[103,"F20"],[104,"F21"],[105,"F22"],[106,"F23"],[107,"F24"],[112,"KanaMode"],[115,"IntlRo"],[119,"Lang4"],[121,"Convert"],[123,"NonConvert"],[125,"IntlYen"],[126,"NumpadComma"],[3597,"NumpadEqual"],[3612,"NumpadEnter"],[3613,"ControlRight"],[3637,"NumpadDivide"],[3639,"PrintScreen"],[3640,"AltRight"],[3653,"Pause"],[3655,"Home"],[3657,"PageUp"],[3663,"End"],[3665,"PageDown"],[3666,"Insert"],[3667,"Delete"],[3675,"MetaLeft"],[3676,"MetaRight"],[3677,"ContextMenu"],[57360,"MediaTrackPrevious"],[57369,"MediaTrackNext"],[57376,"AudioVolumeMute"],[57377,"LaunchApp2"],[57378,"MediaPlayPause"],[57380,"MediaStop"],[57388,"Eject"],[57390,"AudioVolumeDown"],[57392,"AudioVolumeUp"],[57394,"BrowserHome"],[57416,"ArrowUp"],[57419,"ArrowLeft"],[57420,"Numpad5"],[57421,"ArrowRight"],[57424,"ArrowDown"],[57438,"Power"],[57439,"Sleep"],[57443,"WakeUp"],[57445,"BrowserSearch"],[57446,"BrowserFavorites"],[57447,"BrowserRefresh"],[57448,"BrowserStop"],[57449,"BrowserForward"],[57450,"BrowserBack"],[57452,"LaunchMail"],[57453,"MediaSelect"],[60999,"Home"],[61000,"ArrowUp"],[61001,"PageUp"],[61003,"ArrowLeft"],[61004,"Numpad5"],[61005,"ArrowRight"],[61007,"End"],[61008,"ArrowDown"],[61009,"PageDown"],[61010,"Insert"],[61011,"Delete"],[65396,"Open"],[65397,"Help"],[65398,"Props"],[65399,"Select"],[65400,"BrowserStop"],[65401,"Again"],[65402,"Undo"],[65403,"Cut"],[65404,"Copy"],[65405,"Paste"],[65406,"Find"]]"#;

    #[test]
    fn table_matches_the_research_json() {
        let pairs: Vec<(u32, String)> = serde_json::from_str(RESEARCH_JSON).unwrap();
        assert_eq!(pairs.len(), 168);
        for ((code, name), &(c, n)) in pairs.iter().zip(MECHVIBES_KEYCODES.iter()) {
            assert_eq!((*code, name.as_str()), (c, n));
        }
    }

    #[test]
    fn table_is_sorted_and_unique() {
        assert!(MECHVIBES_KEYCODES.windows(2).all(|w| w[0].0 < w[1].0), "sorted, unique");
        let names: HashSet<&str> = MECHVIBES_KEYCODES.iter().map(|&(_, n)| n).collect();
        assert_eq!(names.len(), 155, "distinct W3C names");
    }

    #[test]
    fn known_codes_map_to_the_right_keys() {
        let cases = [
            (1, Key::Escape),
            (14, Key::Backspace),
            (28, Key::Enter),
            (30, Key::KeyA),
            (57, Key::Space),
            (83, Key::NumpadDecimal),
            (91, Key::F13),
            (3612, Key::NumpadEnter),
            (3613, Key::ControlRight),
            (3639, Key::PrintScreen),
            (3640, Key::AltRight),
            (3675, Key::MetaLeft),
            (3677, Key::ContextMenu),
            (57416, Key::ArrowUp),
            (57424, Key::ArrowDown),
            (57420, Key::Numpad5),
            (61000, Key::ArrowUp),
            (61011, Key::Delete),
        ];
        for (code, key) in cases {
            assert!(lookup(code) == Ok(key), "code {code}");
        }
    }

    #[test]
    fn every_name_is_a_taktak_key_or_reported_as_unsupported() {
        let mut unsupported = Vec::new();
        for &(code, name) in &MECHVIBES_KEYCODES {
            match lookup(code) {
                Ok(key) => assert_eq!(key.code_name(), name),
                Err(reason) => {
                    assert!(reason.contains("not a key TakTak supports"), "{code}: {reason}");
                    unsupported.push(name);
                }
            }
        }
        // Media, browser and Sun keys, and Lang4.
        assert_eq!(unsupported.len(), 29, "{unsupported:?}");
        assert!(lookup(84).is_err_and(|r| r == "unknown Mechvibes key code"));
        assert!(lookup(0).is_err());
    }

    #[test]
    fn aliases_and_evdev() {
        assert!(is_windows_alias(60_999) && is_windows_alias(61_011) && !is_windows_alias(61_012));
        assert!(is_secondary(57_420) && !is_secondary(57_416));
        assert_eq!(evdev_name(103), Some("ArrowUp"));
        assert_eq!(evdev_name(30), None);
        assert!(EVDEV_KEYCODES.iter().all(|&(code, _)| (84..=127).contains(&code)));
        assert!(is_strong_evdev_signal(97) && !is_strong_evdev_signal(100));
    }
}
