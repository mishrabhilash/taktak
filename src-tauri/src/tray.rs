//! The menu-bar / tray icon: left click toggles the popover, right click opens the native menu
//! (`Sounds On`, `Pack ▸`, `Mute`, `Settings…`, `Welcome Guide…`, `Quit TakTak`), kept in sync
//! with the state.
//! While sounds are on and not muted by hand but still silent, a status item at the top of the
//! menu and the tooltip say why ([`status`]).
//!
//! Menu items live on the main thread: [`sync`] and the menu handler run there (state changes
//! reach it through `AppHandle::run_on_main_thread`).

use crate::automute;
use crate::service::Service;
use crate::state::{AppState, AudioState, AutoMute, Permission};
use crate::windows;
use std::sync::{Mutex, PoisonError};
use tauri::image::Image;
use tauri::menu::{
    CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu,
};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Runtime};

/// The tray icon's id.
pub const ID: &str = "main";

const MENU_STATUS: &str = "status";
const MENU_ENABLED: &str = "enabled";
const MENU_PACKS: &str = "packs";
const MENU_NO_PACKS: &str = "no-packs";
const MENU_MUTE: &str = "mute";
const MENU_SETTINGS: &str = "settings";
const MENU_WELCOME: &str = "welcome";
const MENU_QUIT: &str = "quit";
/// Pack items are `pack:<id>`.
const PACK_PREFIX: &str = "pack:";

/// Black-on-transparent template image, tinted by macOS for light/dark menu bars.
#[cfg(target_os = "macos")]
const ICON: &[u8] = include_bytes!("../icons/tray-template@2x.png");
/// Colored keycap for the Windows notification area and Linux status bars.
#[cfg(not(target_os = "macos"))]
const ICON: &[u8] = include_bytes!("../icons/tray-color@2x.png");

/// Why TakTak is silent although sounds are on and not muted by hand: the status item's text
/// (escaped) and whether choosing it does something (opens the onboarding window).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub text: String,
    pub actionable: bool,
}

/// The tray's status item for `state`, if any (`docs/ui-contract.md` § Windows, the tray), the
/// first reason that applies: screen locked, output changed, Input Monitoring missing, no sound
/// output, a per-app rule.
pub fn status(state: &AppState) -> Option<Status> {
    status_on(state, cfg!(target_os = "macos"))
}

/// What the status item says while the key listener is missing (`permission` denied): on macOS
/// Input Monitoring; on Linux the `input` group (M5); elsewhere a listener that failed to start
/// and a restart may fix (M5). Each opens the onboarding window, which explains it.
fn denied_text(state: &AppState, mac: bool) -> &'static str {
    if mac {
        "Needs Input Monitoring…"
    } else if state.onboarding.input_group_needed {
        "Needs keyboard access…"
    } else {
        "Key listener stopped…"
    }
}

/// [`status`], with the platform (`mac`) as an argument so both kinds of text are tested.
fn status_on(state: &AppState, mac: bool) -> Option<Status> {
    if !state.settings.enabled || state.muted {
        return None;
    }
    let (text, actionable) = match state.auto_mute {
        Some(AutoMute::ScreenLocked) => ("Muted — screen locked".to_owned(), false),
        Some(AutoMute::OutputChanged) => ("Muted — output device changed".to_owned(), false),
        None if state.permission == Permission::Denied => {
            (denied_text(state, mac).to_owned(), true)
        }
        None if state.audio.state == AudioState::Fault => ("No sound output".to_owned(), false),
        None if state.rule_blocked => {
            let app = state.frontmost_app.as_ref().map_or("this app".into(), |app| {
                taktak_core::pack::printable(&app.name).into_owned()
            });
            (format!("Silent in {app}"), false)
        }
        None => return None,
    };
    Some(Status { text, actionable })
}

/// The tray icon's tooltip: `TakTak`, or `TakTak — <status>` (without a trailing `…`).
pub fn tooltip(status: Option<&Status>) -> String {
    match status {
        Some(status) => format!("TakTak — {}", status.text.trim_end_matches('…')),
        None => "TakTak".to_owned(),
    }
}

/// What the menu shows, from the [`AppState`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MenuModel {
    /// The status item, when there is one.
    pub status: Option<Status>,
    pub enabled: bool,
    /// The mute switch: muted by hand, or auto-muted because the output changed.
    pub muted: bool,
    /// Shown next to `Mute`: the registered hotkey only (not a saved one that failed).
    pub hotkey: Option<String>,
    /// `(id, label)` in the UI's order (by name).
    pub packs: Vec<(String, String)>,
    pub selected: String,
}

impl MenuModel {
    pub fn of(state: &AppState) -> MenuModel {
        MenuModel {
            status: status(state),
            enabled: state.settings.enabled,
            muted: automute::effective_mute(state),
            hotkey: state
                .mute_hotkey_error
                .is_none()
                .then(|| state.settings.mute_hotkey.clone())
                .flatten(),
            packs: state.packs.iter().map(|p| (p.id.clone(), label(&p.name))).collect(),
            selected: state.settings.pack_id.clone(),
        }
    }
}

/// A pack name as a menu label: control characters escaped, and on Windows `&` doubled so it
/// is not taken for a mnemonic.
fn label(name: &str) -> String {
    mnemonic_safe(&taktak_core::pack::printable(name))
}

/// `text` (already escaped) as a menu label: on Windows `&` is doubled.
fn mnemonic_safe(text: &str) -> String {
    if cfg!(target_os = "windows") { text.replace('&', "&&") } else { text.to_owned() }
}

/// What a menu item does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuAction {
    /// The status item (enabled only while the key listener is missing: "Needs Input Monitoring…"
    /// and its Windows and Linux forms): opens the onboarding.
    Status,
    ToggleEnabled,
    ToggleMute,
    SelectPack(String),
    Settings,
    /// `Welcome Guide…`: opens the onboarding window any time (its success state once TakTak
    /// can hear key presses).
    Welcome,
    Quit,
}

impl MenuAction {
    pub fn of(id: &str) -> Option<MenuAction> {
        match id {
            MENU_STATUS => Some(MenuAction::Status),
            MENU_ENABLED => Some(MenuAction::ToggleEnabled),
            MENU_MUTE => Some(MenuAction::ToggleMute),
            MENU_SETTINGS => Some(MenuAction::Settings),
            MENU_WELCOME => Some(MenuAction::Welcome),
            MENU_QUIT => Some(MenuAction::Quit),
            _ => id.strip_prefix(PACK_PREFIX).map(|p| MenuAction::SelectPack(p.to_owned())),
        }
    }
}

/// The live menu items and what they last showed.
struct TrayMenu<R: Runtime> {
    menu: Menu<R>,
    /// The status item and its separator, at the top of `menu` while a status shows.
    status: MenuItem<R>,
    status_separator: PredefinedMenuItem<R>,
    enabled: CheckMenuItem<R>,
    mute: CheckMenuItem<R>,
    packs: Submenu<R>,
    /// The pack list's items: `CheckMenuItem`s, or one disabled "no packs" item.
    pack_items: Vec<(String, CheckMenuItem<R>)>,
    placeholder: Option<MenuItem<R>>,
    shown: MenuModel,
}

/// The tray menu, managed as app state.
pub struct TrayState<R: Runtime>(Mutex<Option<TrayMenu<R>>>);

impl<R: Runtime> TrayMenu<R> {
    /// Brings every item in line with `model`, if it differs from what the menu shows or
    /// `force` is set. After a click, force it: the OS toggles a check item by itself when it is
    /// clicked, whatever the state says.
    fn apply(&mut self, app: &AppHandle<R>, model: &MenuModel, force: bool) -> tauri::Result<()> {
        if !force && *model == self.shown {
            return Ok(());
        }
        if model.status != self.shown.status {
            self.show_status(app, model.status.as_ref())?;
        }
        self.enabled.set_checked(model.enabled)?;
        self.mute.set_checked(model.muted)?;
        if model.hotkey != self.shown.hotkey
            && let Err(e) = self.mute.set_accelerator(model.hotkey.as_deref())
        {
            // The menu's accelerator syntax may not know every hotkey; the hotkey still works.
            log::debug!("mute hotkey not shown in the menu: {e}");
            self.mute.set_accelerator(None::<&str>)?;
        }
        if model.packs != self.shown.packs {
            self.rebuild_packs(app, &model.packs)?;
        }
        for (id, item) in &self.pack_items {
            item.set_checked(*id == model.selected)?;
        }
        self.shown = model.clone();
        Ok(())
    }

    /// Shows `status` at the top of the menu (and in the tooltip), or removes it.
    fn show_status(&mut self, app: &AppHandle<R>, status: Option<&Status>) -> tauri::Result<()> {
        let shown = self.shown.status.is_some();
        match status {
            Some(status) => {
                self.status.set_text(mnemonic_safe(&status.text))?;
                self.status.set_enabled(status.actionable)?;
                if !shown {
                    self.menu.insert(&self.status, 0)?;
                    self.menu.insert(&self.status_separator, 1)?;
                }
            }
            None if shown => {
                self.menu.remove(&self.status_separator)?;
                self.menu.remove(&self.status)?;
            }
            None => {}
        }
        // Absent until the icon is built in `create`, which then sets the first tooltip itself.
        if let Some(tray) = app.tray_by_id(ID) {
            tray.set_tooltip(Some(tooltip(status)))?;
        }
        Ok(())
    }

    fn rebuild_packs(
        &mut self,
        app: &AppHandle<R>,
        packs: &[(String, String)],
    ) -> tauri::Result<()> {
        for (_, item) in self.pack_items.drain(..) {
            self.packs.remove(&item)?;
        }
        if let Some(item) = self.placeholder.take() {
            self.packs.remove(&item)?;
        }
        if packs.is_empty() {
            let item =
                MenuItem::with_id(app, MENU_NO_PACKS, "No sound packs", false, None::<&str>)?;
            self.packs.append(&item)?;
            self.placeholder = Some(item);
            return Ok(());
        }
        for (id, name) in packs {
            let item = CheckMenuItem::with_id(
                app,
                format!("{PACK_PREFIX}{id}"),
                name,
                true,
                false,
                None::<&str>,
            )?;
            self.packs.append(&item)?;
            self.pack_items.push((id.clone(), item));
        }
        Ok(())
    }
}

/// Creates the tray icon and its menu showing `state`. Call once, from `setup`.
pub fn create<R: Runtime>(app: &AppHandle<R>, state: &AppState) -> tauri::Result<()> {
    let status = MenuItem::with_id(app, MENU_STATUS, "", false, None::<&str>)?;
    let status_separator = PredefinedMenuItem::separator(app)?;
    let enabled = CheckMenuItem::with_id(app, MENU_ENABLED, "Sounds On", true, true, None::<&str>)?;
    let packs = Submenu::with_id(app, MENU_PACKS, "Pack", true)?;
    let mute = CheckMenuItem::with_id(app, MENU_MUTE, "Mute", true, false, None::<&str>)?;
    let settings = MenuItem::with_id(app, MENU_SETTINGS, "Settings…", true, None::<&str>)?;
    let welcome = MenuItem::with_id(app, MENU_WELCOME, "Welcome Guide…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, MENU_QUIT, "Quit TakTak", true, None::<&str>)?;
    let items: [&dyn IsMenuItem<R>; 8] = [
        &enabled,
        &packs,
        &mute,
        &PredefinedMenuItem::separator(app)?,
        &settings,
        &welcome,
        &PredefinedMenuItem::separator(app)?,
        &quit,
    ];
    let menu = Menu::with_items(app, &items)?;

    let model = MenuModel::of(state);
    let mut tray_menu = TrayMenu {
        menu: menu.clone(),
        status,
        status_separator,
        enabled,
        mute,
        packs,
        pack_items: Vec::new(),
        placeholder: None,
        // Differs from any real model in the hotkey, so the first apply sets it.
        shown: MenuModel { hotkey: Some(String::new()), ..MenuModel::default() },
    };
    tray_menu.rebuild_packs(app, &[])?;
    tray_menu.apply(app, &model, true)?;
    app.manage(TrayState(Mutex::new(Some(tray_menu))));

    TrayIconBuilder::with_id(ID)
        .icon(Image::from_bytes(ICON)?)
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip(tooltip(model.status.as_ref()))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(on_menu_event)
        .on_tray_icon_event(|tray, event| {
            tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                spawn_window_task(tray.app_handle(), windows::toggle_tray);
            }
        })
        .build(app)?;
    Ok(())
}

/// Shows `state` in the menu. Main thread only.
pub fn sync<R: Runtime>(app: &AppHandle<R>, state: &AppState) {
    show(app, state, false);
}

fn show<R: Runtime>(app: &AppHandle<R>, state: &AppState, force: bool) {
    let Some(tray) = app.try_state::<TrayState<R>>() else { return };
    let mut menu = tray.0.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(menu) = menu.as_mut()
        && let Err(e) = menu.apply(app, &MenuModel::of(state), force)
    {
        log::warn!("cannot update the tray menu: {e}");
    }
}

/// Runs on the main thread when a menu item is chosen.
fn on_menu_event<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    let Some(action) = MenuAction::of(event.id().as_ref()) else { return };
    let Some(service) = app.try_state::<Service>() else { return };
    match action {
        MenuAction::Status => {
            if service.snapshot().permission == Permission::Denied {
                spawn_window_task(app, windows::show_onboarding);
            }
        }
        MenuAction::ToggleEnabled => {
            service.update(|s| automute::set_enabled(s, !s.settings.enabled));
        }
        MenuAction::ToggleMute => {
            service.update(automute::toggle_mute);
        }
        MenuAction::SelectPack(id) => {
            if let Err(e) = service.set_pack(&id) {
                log::warn!("{e}");
            }
        }
        MenuAction::Settings => spawn_window_task(app, windows::show_settings),
        MenuAction::Welcome => spawn_window_task(app, windows::show_onboarding),
        MenuAction::Quit => app.exit(0),
    }
    // A click toggles a check item even when the state did not change (choosing the selected
    // pack again), so show the state again either way.
    show(app, &service.snapshot(), true);
}

/// Runs a window operation off the event-loop thread (see [`windows`]).
pub fn spawn_window_task<R: Runtime>(
    app: &AppHandle<R>,
    task: fn(&AppHandle<R>) -> tauri::Result<()>,
) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = task(&app) {
            log::warn!("window operation failed: {e}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{PackOrigin, PackSummary, Settings};

    fn pack(id: &str, name: &str) -> PackSummary {
        PackSummary {
            id: id.into(),
            name: name.into(),
            author: "A".into(),
            license: "CC0-1.0".into(),
            description: None,
            attribution: None,
            origin: PackOrigin::Bundled,
            has_release: true,
            per_key: false,
            warnings: vec![],
        }
    }

    #[test]
    fn model_follows_the_state() {
        let mut state = AppState::initial("0.1.0", Settings::default());
        state.packs = vec![pack("typewriter", "Typewriter"), pack("evil", "Evil\nName")];
        state.muted = true;
        let model = MenuModel::of(&state);
        assert!(model.enabled && model.muted);
        assert_eq!(model.status, None, "muted by hand: no status item");
        assert_eq!(model.hotkey.as_deref(), Some("CommandOrControl+Alt+Shift+M"));
        assert_eq!(model.selected, "tactile");
        assert_eq!(model.packs[0], ("typewriter".to_owned(), "Typewriter".to_owned()));
        assert_eq!(model.packs[1].1, "Evil\\nName");
    }

    #[test]
    fn a_hotkey_that_failed_to_register_is_not_shown() {
        let mut state = AppState::initial("0.1.0", Settings::default());
        state.mute_hotkey_error = Some("The saved mute hotkey does not work.".into());
        let model = MenuModel::of(&state);
        assert_eq!(model.hotkey, None, "pressing it does nothing, so the menu must not offer it");
        state.mute_hotkey_error = None;
        state.settings.mute_hotkey = None;
        assert_eq!(MenuModel::of(&state).hotkey, None);
    }

    #[test]
    fn the_status_item_says_why_it_is_silent() {
        use crate::automute::Event;
        use crate::service::derive;
        use crate::state::{AppRef, AppRule, AppRuleEntry, AppRuleMode};

        let mut state = AppState::initial("0.1.0", Settings::default());
        state.permission = Permission::Granted;
        state.audio.state = AudioState::Ok;
        assert_eq!(status(&state), None, "playing");
        assert_eq!(tooltip(None), "TakTak");

        // Last reason first: a per-app rule.
        state.rules_supported = true;
        state.settings.app_rule = AppRule {
            mode: AppRuleMode::Never,
            apps: vec![AppRuleEntry { id: "com.x".into(), name: "X".into() }],
        };
        state.frontmost_app = Some(AppRef { id: "com.x".into(), name: "Evil\nApp".into() });
        derive(&mut state);
        let s = status(&state).unwrap();
        assert_eq!(s, Status { text: "Silent in Evil\\nApp".into(), actionable: false });
        assert_eq!(tooltip(Some(&s)), "TakTak — Silent in Evil\\nApp");
        state.settings.app_rule.mode = AppRuleMode::Only;
        state.frontmost_app = None;
        derive(&mut state);
        assert_eq!(status(&state).unwrap().text, "Silent in this app");

        state.audio.state = AudioState::Fault;
        assert_eq!(status(&state).unwrap().text, "No sound output");

        state.permission = Permission::Denied;
        let s = status_on(&state, true).unwrap();
        assert_eq!(s, Status { text: "Needs Input Monitoring…".into(), actionable: true });
        assert_eq!(tooltip(Some(&s)), "TakTak — Needs Input Monitoring");
        // Windows and Linux: no Input Monitoring there (M5).
        state.onboarding.relaunch_suggested = true;
        assert_eq!(status_on(&state, false).unwrap().text, "Key listener stopped…");
        state.onboarding.relaunch_suggested = false;
        state.onboarding.input_group_needed = true;
        let s = status_on(&state, false).unwrap();
        assert_eq!(s, Status { text: "Needs keyboard access…".into(), actionable: true });
        state.onboarding.input_group_needed = false;

        state.settings.mute_on_output_change = true;
        crate::automute::output_changed(&mut state);
        derive(&mut state);
        assert_eq!(status(&state).unwrap().text, "Muted — output device changed");
        assert!(MenuModel::of(&state).muted, "the Mute check shows the output change");

        state.auto_mute_reasons.apply(Event::ScreenLocked);
        derive(&mut state);
        assert_eq!(status(&state).unwrap().text, "Muted — screen locked");

        // Sounds off or muted by hand: no status item at all.
        state.muted = true;
        assert_eq!(status(&state), None);
        state.muted = false;
        state.settings.enabled = false;
        assert_eq!(status(&state), None);
        assert_eq!(MenuModel::of(&state).status, None);
    }

    #[test]
    fn menu_ids_map_to_actions() {
        assert_eq!(MenuAction::of(MENU_STATUS), Some(MenuAction::Status));
        assert_eq!(MenuAction::of(MENU_ENABLED), Some(MenuAction::ToggleEnabled));
        assert_eq!(MenuAction::of(MENU_MUTE), Some(MenuAction::ToggleMute));
        assert_eq!(MenuAction::of(MENU_SETTINGS), Some(MenuAction::Settings));
        assert_eq!(MenuAction::of(MENU_WELCOME), Some(MenuAction::Welcome));
        assert_eq!(MenuAction::of(MENU_QUIT), Some(MenuAction::Quit));
        assert_eq!(MenuAction::of("pack:tactile"), Some(MenuAction::SelectPack("tactile".into())));
        assert_eq!(MenuAction::of(MENU_PACKS), None);
        assert_eq!(MenuAction::of(MENU_NO_PACKS), None);
    }
}
