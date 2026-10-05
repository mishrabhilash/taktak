//! The output device's display name, read without touching the device's input side.
//!
//! cpal's `DeviceTrait::description()` on macOS also counts the device's *input* channels (a
//! stream-configuration query in the input scope) to report a direction. TakTak plays sound
//! only: it never records, and never asks CoreAudio anything about input. So on macOS the name
//! comes from two global-scope reads instead: the default output device, then its name.
//!
//! The device can change between cpal's lookup and this one; the name is for display only, and
//! the engine follows the default device on its own (see `Engine::take_rerouted`).
//!
//! Note: macOS's audio server (coreaudiod) preflights microphone, screen-capture and
//! audio-capture permission for *every* process that connects to it, including output-only
//! ones such as `/usr/bin/afplay`. That preflight is read-only (no prompt, no access) and
//! cannot be avoided by a process that plays sound; see `docs/platform-notes.md`.

#[cfg(target_os = "macos")]
pub fn of(_device: &cpal::Device) -> String {
    macos::default_output_name().unwrap_or_default()
}

#[cfg(not(target_os = "macos"))]
pub fn of(device: &cpal::Device) -> String {
    use cpal::traits::DeviceTrait;
    device.description().map(|d| d.name().to_owned()).unwrap_or_default()
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2_core_audio::{
        AudioObjectGetPropertyData, AudioObjectID, AudioObjectPropertyAddress,
        AudioObjectPropertySelector, kAudioHardwarePropertyDefaultOutputDevice,
        kAudioObjectPropertyElementMain, kAudioObjectPropertyName, kAudioObjectPropertyScopeGlobal,
        kAudioObjectSystemObject, kAudioObjectUnknown,
    };
    use objc2_core_foundation::{CFRetained, CFString};
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::ptr::{self, NonNull};

    /// Reads a global-scope property of `object` into `out`.
    ///
    /// # Safety
    /// `T` must be the exact type CoreAudio writes for `selector` (a plain value or pointer).
    unsafe fn get<T>(
        object: AudioObjectID,
        selector: AudioObjectPropertySelector,
        out: &mut T,
    ) -> bool {
        let address = AudioObjectPropertyAddress {
            mSelector: selector,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain,
        };
        let mut size = size_of::<T>() as u32;
        // SAFETY: `address` and `size` are valid for the call; `out` has room for `size` bytes
        // of the type the caller guarantees CoreAudio writes.
        let status = unsafe {
            AudioObjectGetPropertyData(
                object,
                NonNull::from(&address),
                0,
                ptr::null(),
                NonNull::from(&mut size),
                NonNull::from(out).cast::<c_void>(),
            )
        };
        status == 0 && size as usize == size_of::<T>()
    }

    pub fn default_output_name() -> Option<String> {
        let mut device: AudioObjectID = kAudioObjectUnknown;
        // SAFETY: the default output device property is an `AudioObjectID`.
        let found = unsafe {
            get(
                kAudioObjectSystemObject as AudioObjectID,
                kAudioHardwarePropertyDefaultOutputDevice,
                &mut device,
            )
        };
        if !found || device == kAudioObjectUnknown {
            return None;
        }
        let mut name: *const CFString = ptr::null();
        // SAFETY: `kAudioObjectPropertyName` is a `CFStringRef`.
        if !unsafe { get(device, kAudioObjectPropertyName, &mut name) } {
            return None;
        }
        // SAFETY: the name follows the create rule (+1); `CFRetained` releases it.
        let name = unsafe { CFRetained::from_raw(NonNull::new(name.cast_mut())?) };
        Some(name.to_string())
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn names_the_default_output_device_if_there_is_one() {
            // CI machines may have no output device; when there is one, it has a name.
            if let Some(name) = super::default_output_name() {
                assert!(!name.is_empty());
            }
        }
    }
}
