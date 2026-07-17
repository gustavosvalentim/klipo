use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};

pub fn create(app: &tauri::AppHandle) -> Result<TrayIcon, tauri::Error> {
    let Ok(quit_item) = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>) else {
        panic!("Failed to create quit menu item");
    };

    let Ok(menu) = Menu::with_items(app, &[&quit_item]) else {
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
            println!("Settings clicked");
        }
        _ => {}
    }
}
