use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};

use crate::window::show_settings_window;

pub fn create(app: &tauri::AppHandle) -> Result<TrayIcon, tauri::Error> {
    let Ok(settings_item) = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)
    else {
        panic!("Failed to create settings menu item");
    };
    let Ok(quit_item) = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>) else {
        panic!("Failed to create quit menu item");
    };

    let Ok(menu) = Menu::with_items(app, &[&settings_item, &quit_item]) else {
        panic!("Failed to create menu");
    };

    TrayIconBuilder::new()
        .icon(app.default_window_icon().unwrap().clone())
        .icon_as_template(true)
        .tooltip("Klipo")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(handle_menu_event)
        .build(app)
}

fn handle_menu_event(app: &tauri::AppHandle, event: tauri::menu::MenuEvent) {
    match event.id.as_ref() {
        "quit" => {
            app.exit(0);
        }
        "settings" => {
            if let Err(error) = show_settings_window(app) {
                println!("Failed to show settings: {error}");
            }
        }
        _ => {}
    }
}
