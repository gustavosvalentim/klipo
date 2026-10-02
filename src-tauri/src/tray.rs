use std::sync::Mutex;

use log::error;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Manager};

use crate::window::show_settings_window;

const OPEN_PICKER_MENU_ID: &str = "open-picker";
const SETTINGS_MENU_ID: &str = "settings";
const QUIT_MENU_ID: &str = "quit";
const TRAY_ID: &str = "klipo-tray";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TrayMenuAction {
    OpenPicker,
    Settings,
    Quit,
}

pub struct RetainedTrayIcon(Mutex<Option<TrayIcon>>);

impl RetainedTrayIcon {
    pub fn new(tray_icon: TrayIcon) -> Self {
        Self(Mutex::new(Some(tray_icon)))
    }
}

pub fn create(app: &AppHandle) -> Result<TrayIcon, tauri::Error> {
    let open_picker_item =
        MenuItem::with_id(app, OPEN_PICKER_MENU_ID, "Open Picker", true, None::<&str>)?;
    let settings_item = MenuItem::with_id(app, SETTINGS_MENU_ID, "Settings…", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, QUIT_MENU_ID, "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open_picker_item, &settings_item, &quit_item])?;

    let builder = TrayIconBuilder::with_id(TRAY_ID)
        .icon(
            app.default_window_icon()
                .ok_or(tauri::Error::WindowNotFound)?
                .clone(),
        )
        .tooltip("Klipo")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(handle_menu_event);

    #[cfg(target_os = "macos")]
    let builder = builder.icon_as_template(true);

    builder.build(app)
}

pub fn remove(app: &AppHandle) {
    if let Some(tray) = app.try_state::<RetainedTrayIcon>() {
        remove_tray(&AppTrayManager { app }, &tray.0);
    }
}

fn menu_action(menu_id: &str) -> Option<TrayMenuAction> {
    match menu_id {
        OPEN_PICKER_MENU_ID => Some(TrayMenuAction::OpenPicker),
        SETTINGS_MENU_ID => Some(TrayMenuAction::Settings),
        QUIT_MENU_ID => Some(TrayMenuAction::Quit),
        _ => None,
    }
}

fn take_retained<T>(retained: &Mutex<Option<T>>) -> Option<T> {
    retained
        .lock()
        .ok()
        .and_then(|mut retained_resource| retained_resource.take())
}

trait TrayManager {
    fn remove_tray_by_id(&self, tray_id: &str);
}

struct AppTrayManager<'a> {
    app: &'a AppHandle,
}

impl TrayManager for AppTrayManager<'_> {
    fn remove_tray_by_id(&self, tray_id: &str) {
        drop(self.app.remove_tray_by_id(tray_id));
    }
}

fn remove_tray<T>(manager: &impl TrayManager, retained_tray: &Mutex<Option<T>>) {
    if let Some(retained_icon) = take_retained(retained_tray) {
        manager.remove_tray_by_id(TRAY_ID);
        drop(retained_icon);
    }
}

fn handle_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    let action = menu_action(event.id.as_ref());
    let action_result = match action {
        Some(TrayMenuAction::OpenPicker) => crate::picker::open(app),
        Some(TrayMenuAction::Settings) => {
            show_settings_window(app).map_err(|error_value| error_value.to_string())
        }
        Some(TrayMenuAction::Quit) => {
            crate::commands::exit_application(app);
            Ok(())
        }
        None => Ok(()),
    };
    if let Err(error_value) = action_result {
        error!(action:? = action, error:% = error_value; "Failed to handle tray menu action");
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    struct FakeTrayManager<'a> {
        events: &'a RefCell<Vec<&'static str>>,
    }

    impl TrayManager for FakeTrayManager<'_> {
        fn remove_tray_by_id(&self, tray_id: &str) {
            assert_eq!(tray_id, TRAY_ID);
            self.events.borrow_mut().push("managed tray");
        }
    }

    struct RetainedTestTray<'a> {
        events: &'a RefCell<Vec<&'static str>>,
    }

    impl Drop for RetainedTestTray<'_> {
        fn drop(&mut self) {
            self.events.borrow_mut().push("retained tray");
        }
    }

    #[test]
    fn removes_the_managed_tray_before_dropping_the_retained_clone() {
        let events = RefCell::new(Vec::new());
        let retained = Mutex::new(Some(RetainedTestTray { events: &events }));
        let manager = FakeTrayManager { events: &events };

        remove_tray(&manager, &retained);
        remove_tray(&manager, &retained);

        assert_eq!(
            events.borrow().as_slice(),
            ["managed tray", "retained tray"]
        );
    }

    #[test]
    fn maps_each_tray_menu_item_to_its_action() {
        assert_eq!(
            menu_action(OPEN_PICKER_MENU_ID),
            Some(TrayMenuAction::OpenPicker)
        );
        assert_eq!(
            menu_action(SETTINGS_MENU_ID),
            Some(TrayMenuAction::Settings)
        );
        assert_eq!(menu_action(QUIT_MENU_ID), Some(TrayMenuAction::Quit));
        assert_eq!(menu_action("unknown"), None);
    }
}
