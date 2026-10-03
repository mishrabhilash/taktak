//! Auto-mute (`docs/ui-contract.md` § Auto-mute): TakTak silences itself while the screen is
//! locked or the user session is inactive, and, when the user asked for it, after the default
//! output device changes. Pure state machine, unit-tested; the platform observers are in
//! `apps.rs` and the output device is watched by the control thread (`service.rs`).
//!
//! Each reason is tracked separately and none of them touches the manual mute (`muted`) or
//! `settings.enabled`, so auto-mute never fights them: sound plays only when no reason applies.

use crate::state::{AppState, AutoMute};

/// Why TakTak is auto-muted, each reason on its own. Kept in [`AppState`] (not serialized);
/// `AppState::auto_mute` shows the strongest one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AutoMuteReasons {
    /// `com.apple.screenIsLocked` arrived and `com.apple.screenIsUnlocked` not yet.
    pub screen_locked: bool,
    /// This user session resigned active (fast user switching) and has not come back yet.
    pub session_inactive: bool,
    /// The default output device changed while `mute_on_output_change` was armed; stays until
    /// the user unmutes, turns sounds on or turns the setting off.
    pub output_changed: bool,
}

/// What can change an auto-mute reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    ScreenLocked,
    ScreenUnlocked,
    /// `NSWorkspaceSessionDidResignActiveNotification`.
    SessionInactive,
    /// `NSWorkspaceSessionDidBecomeActiveNotification`.
    SessionActive,
    /// The default output device became a different device; `armed` = [`armed`] at that moment.
    OutputChanged {
        armed: bool,
    },
    /// The user set the manual mute, either way (`set_muted`, the hotkey, the tray `Mute`).
    ManualMute,
    /// The user turned sounds on.
    SoundsOn,
    /// The user turned "Mute when the sound output changes" off.
    SettingOff,
}

impl AutoMuteReasons {
    /// Applies `event`. Nothing the user does clears `screenLocked`; only unlocking does.
    pub fn apply(&mut self, event: Event) {
        match event {
            Event::ScreenLocked => self.screen_locked = true,
            Event::ScreenUnlocked => self.screen_locked = false,
            Event::SessionInactive => self.session_inactive = true,
            Event::SessionActive => self.session_inactive = false,
            Event::OutputChanged { armed } => self.output_changed |= armed,
            Event::ManualMute | Event::SoundsOn | Event::SettingOff => self.output_changed = false,
        }
    }

    /// The reason to report: `screenLocked` (lock or inactive session) before `outputChanged`.
    pub fn reason(&self) -> Option<AutoMute> {
        if self.screen_locked || self.session_inactive {
            Some(AutoMute::ScreenLocked)
        } else if self.output_changed {
            Some(AutoMute::OutputChanged)
        } else {
            None
        }
    }
}

/// Whether an output device change sets `outputChanged` now: the setting is on and sounds are
/// on and not muted by hand (a lock does not matter).
pub fn armed(mute_on_output_change: bool, enabled: bool, muted: bool) -> bool {
    mute_on_output_change && enabled && !muted
}

/// The mute switch the UI and the tray show, and the hotkey toggles: the manual mute, or an
/// `outputChanged` auto-mute (which only an unmute clears).
pub fn effective_mute(state: &AppState) -> bool {
    state.muted || state.auto_mute_reasons.output_changed
}

/// Sets the manual mute (`set_muted`): either value clears `outputChanged` (the manual mute
/// takes over). `screenLocked` stays.
pub fn set_muted(state: &mut AppState, muted: bool) {
    state.muted = muted;
    state.auto_mute_reasons.apply(Event::ManualMute);
}

/// Toggles the [`effective_mute`] (the hotkey and the tray `Mute` item): unmutes when it is
/// on, clearing both, and mutes otherwise.
pub fn toggle_mute(state: &mut AppState) {
    let on = effective_mute(state);
    set_muted(state, !on);
}

/// `set_enabled`: turning sounds on also clears `outputChanged`.
pub fn set_enabled(state: &mut AppState, enabled: bool) {
    state.settings.enabled = enabled;
    if enabled {
        state.auto_mute_reasons.apply(Event::SoundsOn);
    }
}

/// `set_mute_on_output_change`: turning it off also clears `outputChanged`.
pub fn set_mute_on_output_change(state: &mut AppState, enabled: bool) {
    state.settings.mute_on_output_change = enabled;
    if !enabled {
        state.auto_mute_reasons.apply(Event::SettingOff);
    }
}

/// The default output device became another device.
pub fn output_changed(state: &mut AppState) {
    let armed = armed(state.settings.mute_on_output_change, state.settings.enabled, state.muted);
    state.auto_mute_reasons.apply(Event::OutputChanged { armed });
}

/// The default output device as TakTak last saw it, to tell a change of device from the first
/// device after launch and from the same device coming back after a fault or a gap.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeviceWatch {
    last: Option<String>,
}

impl DeviceWatch {
    /// Records the default output device (`None` or empty: none, or unnamed, which is ignored).
    /// Returns whether it is a different device than the one seen last.
    pub fn see(&mut self, name: Option<&str>) -> bool {
        let Some(name) = name.filter(|n| !n.is_empty()) else { return false };
        if self.last.as_deref() == Some(name) {
            return false;
        }
        self.last.replace(name.to_owned()).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Settings;

    fn state() -> AppState {
        AppState::initial("0.1.0", Settings { mute_on_output_change: true, ..Settings::default() })
    }

    #[test]
    fn locking_mutes_until_both_lock_and_session_are_undone() {
        let mut r = AutoMuteReasons::default();
        assert_eq!(r.reason(), None);
        r.apply(Event::ScreenLocked);
        assert_eq!(r.reason(), Some(AutoMute::ScreenLocked));
        r.apply(Event::SessionInactive);
        r.apply(Event::ScreenUnlocked);
        assert_eq!(r.reason(), Some(AutoMute::ScreenLocked), "the session is still inactive");
        r.apply(Event::SessionActive);
        assert_eq!(r.reason(), None);
        // Nothing the user does clears a lock.
        r.apply(Event::ScreenLocked);
        for event in [Event::ManualMute, Event::SoundsOn, Event::SettingOff] {
            r.apply(event);
            assert_eq!(r.reason(), Some(AutoMute::ScreenLocked), "{event:?}");
        }
    }

    #[test]
    fn an_output_change_mutes_only_when_armed_and_outlasts_a_lock() {
        let mut r = AutoMuteReasons::default();
        r.apply(Event::OutputChanged { armed: false });
        assert_eq!(r.reason(), None);
        r.apply(Event::OutputChanged { armed: true });
        assert_eq!(r.reason(), Some(AutoMute::OutputChanged));
        // A later unarmed change does not clear it.
        r.apply(Event::OutputChanged { armed: false });
        assert_eq!(r.reason(), Some(AutoMute::OutputChanged));
        // Locking shows the stronger reason; the output change is still there after unlocking.
        r.apply(Event::ScreenLocked);
        assert_eq!(r.reason(), Some(AutoMute::ScreenLocked));
        r.apply(Event::ScreenUnlocked);
        assert_eq!(r.reason(), Some(AutoMute::OutputChanged));
        for event in [Event::ManualMute, Event::SoundsOn, Event::SettingOff] {
            let mut r = r;
            r.apply(event);
            assert_eq!(r.reason(), None, "{event:?} clears it");
        }
        // Lock and session events leave it alone.
        for event in [Event::SessionActive, Event::ScreenUnlocked] {
            let mut r = r;
            r.apply(event);
            assert_eq!(r.reason(), Some(AutoMute::OutputChanged), "{event:?}");
        }
    }

    #[test]
    fn arming_needs_the_setting_sounds_on_and_no_manual_mute() {
        assert!(armed(true, true, false));
        assert!(!armed(false, true, false), "setting off");
        assert!(!armed(true, false, false), "sounds off");
        assert!(!armed(true, true, true), "muted by hand");
    }

    #[test]
    fn the_mute_switch_shows_and_toggles_the_effective_mute() {
        let mut s = state();
        assert!(!effective_mute(&s));
        output_changed(&mut s);
        assert!(effective_mute(&s) && !s.muted, "auto-muted, manual mute untouched");
        // The hotkey or tray: the switch is on, so it unmutes, clearing both.
        toggle_mute(&mut s);
        assert!(!effective_mute(&s) && !s.muted);
        toggle_mute(&mut s);
        assert!(s.muted && !s.auto_mute_reasons.output_changed);
        toggle_mute(&mut s);
        assert!(!s.muted);

        // set_muted(true) while auto-muted: the manual mute takes over.
        output_changed(&mut s);
        set_muted(&mut s, true);
        assert!(s.muted && !s.auto_mute_reasons.output_changed);
        // Muted by hand: a device change does not arm.
        output_changed(&mut s);
        assert!(!s.auto_mute_reasons.output_changed);
        set_muted(&mut s, false);

        // Toggling never touches a lock.
        s.auto_mute_reasons.apply(Event::ScreenLocked);
        toggle_mute(&mut s);
        toggle_mute(&mut s);
        assert!(s.auto_mute_reasons.screen_locked);
    }

    #[test]
    fn sounds_on_and_the_setting_off_clear_an_output_change() {
        let mut s = state();
        output_changed(&mut s);
        set_enabled(&mut s, false);
        assert!(s.auto_mute_reasons.output_changed, "turning sounds off keeps it");
        set_enabled(&mut s, true);
        assert!(!s.auto_mute_reasons.output_changed);

        output_changed(&mut s);
        set_mute_on_output_change(&mut s, true);
        assert!(s.auto_mute_reasons.output_changed);
        set_mute_on_output_change(&mut s, false);
        assert!(!s.auto_mute_reasons.output_changed && !s.settings.mute_on_output_change);
        output_changed(&mut s);
        assert!(!s.auto_mute_reasons.output_changed, "not armed with the setting off");
    }

    #[test]
    fn devices_change_by_name_but_not_on_first_sight_or_return() {
        let mut watch = DeviceWatch::default();
        assert!(!watch.see(Some("MacBook Pro Speakers")), "the first device after launch");
        assert!(!watch.see(Some("MacBook Pro Speakers")));
        assert!(watch.see(Some("AirPods Pro")));
        // A fault or no device, then the same device back: not a change.
        assert!(!watch.see(None));
        assert!(!watch.see(Some("")));
        assert!(!watch.see(Some("AirPods Pro")));
        assert!(watch.see(Some("MacBook Pro Speakers")));
    }
}
