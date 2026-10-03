//! The menu-bar / tray icon: left click toggles the popover, right click opens the native menu
//! (`Sounds On`, `Pack ▸`, `Mute`, `Settings…`, `Quit TakTak`), kept in sync with the state.
//!
//! Menu items live on the main thread: [`sync`] and the menu handler run there (state changes
//! reach it through `AppHandle::run_on_main_thread`).

use crate::service::Service;
use crate::state::AppState;
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

const MENU_ENABLED: &str = "enabled";
const MENU_PACKS: &str = "packs";
const MENU_NO_PACKS: &str = "no-packs";
const MENU_MUTE: &str = "mute";
const MENU_SETTINGS: &str = "settings";
const MENU_QUIT: &str = "quit";
/// Pack items are `pack:<id>`.
const PACK_PREFIX: &str = "pack:";

/// Black-on-transparent template image, tinted by macOS for light/dark menu bars.
#[cfg(target_os = "macos")]
const ICON: &[u8] = include_bytes!("../icons/tray-template@2x.png");
/// Colored keycap for the Windows notification area and Linux status bars.
#[cfg(not(target_os = "macos"))]
const ICON: &[u8] = include_bytes!("../icons/tray-color@2x.png");

/// What the menu shows, from the [`AppState`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MenuModel {
    pub enabled: bool,
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
            enabled: state.settings.enabled,
            muted: state.muted,
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
    let text = taktak_core::pack::printable(name);
    if cfg!(target_os = "windows") { text.replace('&', "&&") } else { text.into_owned() }
}

/// What a menu item does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuAction {
    ToggleEnabled,
    ToggleMute,
    SelectPack(String),
    Settings,
    Quit,
}

impl MenuAction {
    pub fn of(id: &str) -> Option<MenuAction> {
        match id {
            MENU_ENABLED => Some(MenuAction::ToggleEnabled),
            MENU_MUTE => Some(MenuAction::ToggleMute),
            MENU_SETTINGS => Some(MenuAction::Settings),
            MENU_QUIT => Some(MenuAction::Quit),
            _ => id.strip_prefix(PACK_PREFIX).map(|p| MenuAction::SelectPack(p.to_owned())),
        }
    }
}

/// The live menu items and what they last showed.
struct TrayMenu<R: Runtime> {
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
    let enabled = CheckMenuItem::with_id(app, MENU_ENABLED, "Sounds On", true, true, None::<&str>)?;
    let packs = Submenu::with_id(app, MENU_PACKS, "Pack", true)?;
    let mute = CheckMenuItem::with_id(app, MENU_MUTE, "Mute", true, false, None::<&str>)?;
    let settings = MenuItem::with_id(app, MENU_SETTINGS, "Settings…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, MENU_QUIT, "Quit TakTak", true, None::<&str>)?;
    let items: [&dyn IsMenuItem<R>; 7] = [
        &enabled,
        &packs,
        &mute,
        &PredefinedMenuItem::separator(app)?,
        &settings,
        &PredefinedMenuItem::separator(app)?,
        &quit,
    ];
    let menu = Menu::with_items(app, &items)?;

    let mut tray_menu = TrayMenu {
        enabled,
        mute,
        packs,
        pack_items: Vec::new(),
        placeholder: None,
        // Differs from any real model in the hotkey, so the first apply sets it.
        shown: MenuModel { hotkey: Some(String::new()), ..MenuModel::default() },
    };
    tray_menu.rebuild_packs(app, &[])?;
    tray_menu.apply(app, &MenuModel::of(state), true)?;
    app.manage(TrayState(Mutex::new(Some(tray_menu))));

    TrayIconBuilder::with_id(ID)
        .icon(Image::from_bytes(ICON)?)
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip("TakTak")
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
        MenuAction::ToggleEnabled => {
            service.update(|s| s.settings.enabled = !s.settings.enabled);
        }
        MenuAction::ToggleMute => {
            service.update(|s| s.muted = !s.muted);
        }
        MenuAction::SelectPack(id) => {
            if let Err(e) = service.set_pack(&id) {
                log::warn!("{e}");
            }
        }
        MenuAction::Settings => spawn_window_task(app, windows::show_settings),
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
        state.packs = vec![pack("blue-click", "Blue Click"), pack("evil", "Evil\nName")];
        state.muted = true;
        let model = MenuModel::of(&state);
        assert!(model.enabled && model.muted);
        assert_eq!(model.hotkey.as_deref(), Some("CommandOrControl+Alt+Shift+M"));
        assert_eq!(model.selected, "deep-thock");
        assert_eq!(model.packs[0], ("blue-click".to_owned(), "Blue Click".to_owned()));
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
    fn menu_ids_map_to_actions() {
        assert_eq!(MenuAction::of(MENU_ENABLED), Some(MenuAction::ToggleEnabled));
        assert_eq!(MenuAction::of(MENU_MUTE), Some(MenuAction::ToggleMute));
        assert_eq!(MenuAction::of(MENU_SETTINGS), Some(MenuAction::Settings));
        assert_eq!(MenuAction::of(MENU_QUIT), Some(MenuAction::Quit));
        assert_eq!(
            MenuAction::of("pack:deep-thock"),
            Some(MenuAction::SelectPack("deep-thock".into()))
        );
        assert_eq!(MenuAction::of(MENU_PACKS), None);
        assert_eq!(MenuAction::of(MENU_NO_PACKS), None);
    }
}
