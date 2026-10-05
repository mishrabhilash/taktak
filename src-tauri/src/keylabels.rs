//! Keycap labels for hotkeys on the current keyboard layout (macOS): `key_labels` and the
//! `key-labels-changed` event.
//!
//! On macOS the global-shortcut plugin registers an accelerator's key as a key *position* (a
//! virtual key code named after the US layout), so "M" in ⌘⌥⇧M is the key right of N, which
//! prints "," on French AZERTY. To show the right character, this module asks the current
//! keyboard layout (`TISCopyCurrentKeyboardLayoutInputSource`, its `'uchr'` data and
//! `UCKeyTranslate`, base layer, no modifiers) what each of those fixed positions prints, and
//! hands the UI a map from `KeyboardEvent.code` to that label. It is read once at startup and
//! again whenever the user switches input source (the distributed notification
//! `kTISNotifySelectedKeyboardInputSourceChanged`, event-driven, never polled), on the main
//! thread.
//!
//! Privacy: this translates a fixed list of key positions for display only. It never sees, asks
//! about or translates keys the user types; nothing is logged or stored. The key hook still
//! calls no layout or character API.
//!
//! Windows and Linux: there the plugin registers the key that types the token's character
//! already, so there is nothing to translate; `current` is `None` and the UI keeps its own names.

use std::collections::BTreeMap;
use taktak_core::input::keymap_macos;
use taktak_core::key::Key;

/// `KeyboardEvent.code` → the label the current layout prints on that key.
pub type KeyLabels = BTreeMap<String, String>;

/// The event sent to every window when the labels change (payload: [`KeyLabels`] or null).
pub const CHANGED: &str = "key-labels-changed";

/// Whether `key` is a key an accelerator can name whose printed character depends on the
/// layout: letters, digits and punctuation (as `isCharacterKey` in `src/lib/accelerator.ts`).
pub fn is_layout_key(key: Key) -> bool {
    use Key::*;
    let name = key.code_name();
    (name.len() == 4 && name.starts_with("Key"))
        || (name.len() == 6 && name.starts_with("Digit"))
        || matches!(
            key,
            Minus
                | Equal
                | BracketLeft
                | BracketRight
                | Backslash
                | Semicolon
                | Quote
                | Comma
                | Period
                | Slash
                | Backquote
        )
}

/// The macOS virtual key code of every layout-dependent key an accelerator can name, with its
/// `KeyboardEvent.code`: the fixed list [`current`] translates.
pub fn layout_keys() -> Vec<(u16, &'static str)> {
    (0..0x80u16)
        .filter_map(|code| keymap_macos::from_virtual_keycode(code).map(|key| (code, key)))
        .filter(|&(_, key)| is_layout_key(key))
        .map(|(code, key)| (code, key.code_name()))
        .collect()
}

/// The label for `UCKeyTranslate`'s UTF-16 output: `None` when it is empty, not valid UTF-16,
/// or not something visible (whitespace, control characters).
pub fn label_from_utf16(units: &[u16]) -> Option<String> {
    let text = String::from_utf16(units).ok()?;
    let text = text.trim();
    if text.is_empty() || text.chars().any(char::is_control) {
        return None;
    }
    Some(text.to_owned())
}

/// Builds the map from each key's translation (`translate` gets a virtual key code and returns
/// its UTF-16 output, `None` on failure); `None` when nothing translated.
pub fn labels_from(mut translate: impl FnMut(u16) -> Option<Vec<u16>>) -> Option<KeyLabels> {
    let labels: KeyLabels = layout_keys()
        .into_iter()
        .filter_map(|(code, name)| {
            let label = label_from_utf16(&translate(code)?)?;
            Some((name.to_owned(), label))
        })
        .collect();
    (!labels.is_empty()).then_some(labels)
}

pub use platform::{current, read_installed, read_now, stop_watching, watch};

#[cfg(target_os = "macos")]
mod platform {
    use super::KeyLabels;
    use objc2::MainThreadMarker;
    use objc2_core_foundation::{
        CFDictionary, CFNotificationCenter, CFNotificationName, CFNotificationSuspensionBehavior,
        CFRetained, CFString,
    };
    use std::ffi::c_void;
    use std::sync::{Mutex, MutexGuard, PoisonError};

    type Listener = Box<dyn Fn(Option<KeyLabels>) + Send>;

    /// The labels for the layout in use, read on the main thread.
    static CURRENT: Mutex<Option<KeyLabels>> = Mutex::new(None);
    /// Told about every change.
    static LISTENER: Mutex<Option<Listener>> = Mutex::new(None);
    /// The distributed center while watching (main thread only).
    static CENTER: Mutex<Option<SendCenter>> = Mutex::new(None);
    /// The observer's identity in the distributed center: an address unique to this module.
    static OBSERVER: u8 = 0;

    struct SendCenter(CFRetained<CFNotificationCenter>);
    // SAFETY: only used on the main thread (registered in `watch`, removed in `stop_watching`).
    unsafe impl Send for SendCenter {}

    fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
        mutex.lock().unwrap_or_else(PoisonError::into_inner)
    }

    const KUC_KEY_ACTION_DISPLAY: u16 = 3;
    const KUC_KEY_TRANSLATE_NO_DEAD_KEYS_MASK: u32 = 1;

    #[link(name = "Carbon", kind = "framework")]
    unsafe extern "C" {
        fn TISCopyCurrentKeyboardLayoutInputSource() -> *mut c_void;
        fn TISCopyCurrentASCIICapableKeyboardLayoutInputSource() -> *mut c_void;
        fn TISGetInputSourceProperty(source: *mut c_void, key: *const c_void) -> *const c_void;
        fn TISCreateInputSourceList(properties: *const c_void, include_all: u8) -> *const c_void;
        static kTISPropertyUnicodeKeyLayoutData: *const c_void;
        static kTISPropertyInputSourceID: *const c_void;
        static kTISNotifySelectedKeyboardInputSourceChanged: *const c_void;
        fn LMGetKbdType() -> u8;
        fn UCKeyTranslate(
            layout: *const c_void,
            virtual_key_code: u16,
            key_action: u16,
            modifier_key_state: u32,
            keyboard_type: u32,
            key_translate_options: u32,
            dead_key_state: *mut u32,
            max_string_length: usize,
            actual_string_length: *mut usize,
            unicode_string: *mut u16,
        ) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFDataGetBytePtr(data: *const c_void) -> *const u8;
        fn CFArrayGetCount(array: *const c_void) -> isize;
        fn CFArrayGetValueAtIndex(array: *const c_void, index: isize) -> *const c_void;
        fn CFRelease(cf: *const c_void);
    }

    /// Translates the fixed key positions with `source`'s `'uchr'` layout, if it has one.
    /// SAFETY: `source` is a valid input source; main thread.
    unsafe fn translate_with(source: *mut c_void) -> Option<KeyLabels> {
        if source.is_null() {
            return None;
        }
        // SAFETY: a valid source and a Carbon constant; the data is owned by the source.
        let data = unsafe { TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData) };
        if data.is_null() {
            return None;
        }
        // SAFETY: `data` is a CFData holding a UCKeyboardLayout, alive as long as `source`.
        let layout = unsafe { CFDataGetBytePtr(data) }.cast::<c_void>();
        if layout.is_null() {
            return None;
        }
        // SAFETY: no preconditions.
        let keyboard_type = u32::from(unsafe { LMGetKbdType() });
        super::labels_from(|code| {
            let mut dead_keys = 0u32;
            let mut chars = [0u16; 8];
            let mut len = 0usize;
            // SAFETY: a valid layout, buffers of the stated length; no modifiers (base layer),
            // dead keys give their own character.
            let status = unsafe {
                UCKeyTranslate(
                    layout,
                    code,
                    KUC_KEY_ACTION_DISPLAY,
                    0,
                    keyboard_type,
                    KUC_KEY_TRANSLATE_NO_DEAD_KEYS_MASK,
                    &mut dead_keys,
                    chars.len(),
                    &mut len,
                    chars.as_mut_ptr(),
                )
            };
            (status == 0).then(|| chars[..len.min(chars.len())].to_vec())
        })
    }

    /// Reads the labels of the layout in use. Main thread only (`None` elsewhere). Input
    /// methods without a key layout of their own (Japanese, Chinese…) use the ASCII-capable
    /// layout they type through.
    fn read(_mtm: MainThreadMarker) -> Option<KeyLabels> {
        // SAFETY (each block): TIS calls on the main thread; every copied source is released.
        unsafe {
            let source = TISCopyCurrentKeyboardLayoutInputSource();
            let labels = translate_with(source);
            if !source.is_null() {
                CFRelease(source);
            }
            if labels.is_some() {
                return labels;
            }
            let ascii = TISCopyCurrentASCIICapableKeyboardLayoutInputSource();
            let labels = translate_with(ascii);
            if !ascii.is_null() {
                CFRelease(ascii);
            }
            labels
        }
    }

    /// Reads the labels of the installed keyboard layout `id` (e.g. `com.apple.keylayout.French`)
    /// without selecting it: the self-test's check that the translation reads a non-US layout.
    /// Main thread only.
    pub fn read_installed(id: &str) -> Option<KeyLabels> {
        MainThreadMarker::new()?;
        // SAFETY: TIS calls on the main thread. The list is released; its sources are owned by
        // it and only used before that. Property values are the sources' own (not released).
        unsafe {
            let list = TISCreateInputSourceList(std::ptr::null(), 1);
            if list.is_null() {
                return None;
            }
            let mut labels = None;
            for i in 0..CFArrayGetCount(list) {
                let source = CFArrayGetValueAtIndex(list, i).cast_mut();
                let source_id = TISGetInputSourceProperty(source, kTISPropertyInputSourceID);
                if source_id.is_null() {
                    continue;
                }
                if (*source_id.cast::<CFString>()).to_string() == id {
                    labels = translate_with(source);
                    break;
                }
            }
            CFRelease(list);
            labels
        }
    }

    /// Reads the labels of the layout in use now, without watching (the self-test). `None` off
    /// the main thread or when the layout cannot be read.
    pub fn read_now() -> Option<KeyLabels> {
        MainThreadMarker::new().and_then(read)
    }

    /// The labels for the layout in use, as last read (at [`watch`] and on every input-source
    /// change); `None` before [`watch`] or when the layout cannot be read.
    pub fn current() -> Option<KeyLabels> {
        lock(&CURRENT).clone()
    }

    fn refresh(mtm: MainThreadMarker) {
        let labels = read(mtm);
        let changed = {
            let mut current = lock(&CURRENT);
            let changed = *current != labels;
            current.clone_from(&labels);
            changed
        };
        if changed && let Some(listener) = lock(&LISTENER).as_ref() {
            listener(labels);
        }
    }

    unsafe extern "C-unwind" fn on_input_source_changed(
        _center: *mut CFNotificationCenter,
        _observer: *mut c_void,
        _name: *const CFNotificationName,
        _object: *const c_void,
        _info: *const CFDictionary,
    ) {
        // Delivered on the main run loop, where `watch` registered it.
        if let Some(mtm) = MainThreadMarker::new() {
            refresh(mtm);
        }
    }

    /// Reads the labels now and again on every input-source change, telling `on_change` about
    /// each change. Main thread only (does nothing elsewhere). Idempotent.
    pub fn watch(on_change: impl Fn(Option<KeyLabels>) + Send + 'static) {
        let Some(mtm) = MainThreadMarker::new() else {
            log::warn!("keyboard layout labels are off: not on the main thread");
            return;
        };
        *lock(&LISTENER) = Some(Box::new(on_change));
        refresh(mtm);
        let mut center = lock(&CENTER);
        if center.is_some() {
            return;
        }
        let Some(distributed) = CFNotificationCenter::distributed_center() else {
            log::warn!("cannot follow keyboard layout changes: no distributed notification center");
            return;
        };
        // SAFETY: a Carbon constant CFString.
        let name: &CFString =
            unsafe { &*kTISNotifySelectedKeyboardInputSourceChanged.cast::<CFString>() };
        // SAFETY: a static observer address, a callback with the right signature; removed in
        // `stop_watching`. Immediate delivery: TakTak is rarely the active app.
        unsafe {
            distributed.add_observer(
                std::ptr::from_ref(&OBSERVER).cast(),
                Some(on_input_source_changed),
                Some(name),
                std::ptr::null(),
                CFNotificationSuspensionBehavior::DeliverImmediately,
            );
        }
        *center = Some(SendCenter(distributed));
    }

    /// Stops following input-source changes (on quit). Main thread.
    pub fn stop_watching() {
        if let Some(SendCenter(center)) = lock(&CENTER).take() {
            // SAFETY: the observer registered in `watch`.
            unsafe { center.remove_every_observer(std::ptr::from_ref(&OBSERVER).cast()) };
        }
        *lock(&LISTENER) = None;
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::KeyLabels;

    /// Always `None`: the hotkey's key is the character itself here.
    pub fn current() -> Option<KeyLabels> {
        None
    }

    pub fn read_now() -> Option<KeyLabels> {
        None
    }

    pub fn read_installed(_id: &str) -> Option<KeyLabels> {
        None
    }

    pub fn watch(_on_change: impl Fn(Option<KeyLabels>) + Send + 'static) {}

    pub fn stop_watching() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_exactly_the_layout_dependent_accelerator_keys() {
        let keys = layout_keys();
        // 26 letters, 10 digits, 11 punctuation keys; each once.
        assert_eq!(keys.len(), 47);
        let mut names: Vec<&str> = keys.iter().map(|&(_, name)| name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 47);
        // kVK_ANSI_M, kVK_ANSI_Semicolon, kVK_ANSI_Grave.
        assert!(keys.contains(&(0x2E, "KeyM")));
        assert!(keys.contains(&(0x29, "Semicolon")));
        assert!(keys.contains(&(0x32, "Backquote")));
        // Never keys the layout does not change, nor the ISO key accelerators cannot name.
        for name in ["Space", "Enter", "F1", "Numpad1", "IntlBackslash", "ArrowUp"] {
            assert!(!names.contains(&name), "{name}");
        }
    }

    #[test]
    fn labels_are_visible_text_only() {
        let utf16 = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
        assert_eq!(label_from_utf16(&utf16(",")), Some(",".into()));
        assert_eq!(label_from_utf16(&utf16("ß")), Some("ß".into()));
        assert_eq!(label_from_utf16(&utf16("^")), Some("^".into()), "a dead key's own sign");
        assert_eq!(label_from_utf16(&utf16("😀")), Some("😀".into()), "a surrogate pair");
        assert_eq!(label_from_utf16(&[]), None);
        assert_eq!(label_from_utf16(&utf16(" ")), None);
        assert_eq!(label_from_utf16(&[0x0010]), None, "a control character");
        assert_eq!(label_from_utf16(&[0xD800]), None, "a lone surrogate");
    }

    #[test]
    fn builds_the_map_from_each_translation() {
        // A pretend AZERTY: M's position prints ",", Q's prints "a", others fail or are blank.
        let labels = labels_from(|code| match code {
            0x2E => Some(",".encode_utf16().collect()),
            0x0C => Some("a".encode_utf16().collect()),
            0x00 => Some(vec![]),
            _ => None,
        })
        .unwrap();
        assert_eq!(labels.len(), 2);
        assert_eq!(labels["KeyM"], ",");
        assert_eq!(labels["KeyQ"], "a");
        assert_eq!(labels_from(|_| None), None, "nothing translated");
    }

    #[test]
    fn reads_the_layout_on_macos_only() {
        // Not watching in tests: nothing read yet anywhere.
        assert_eq!(current(), None);
    }
}
