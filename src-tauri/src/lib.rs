mod clipboard;
mod commands;
mod input;
mod settings;
mod shortcuts;
mod state;
mod tray;
mod window;

use clipboard::ClipboardEventsListener;
use commands::{
    clear, close, delete_item, fetch_clipboard, get_shortcuts, paste, quit, save_shortcuts,
};
use shortcuts::{load_and_register_shortcuts, register_shortcuts_plugin};
use state::AppState;
use window::{create_klipo_window, window_events_handler};

const WINDOW_WIDTH: f64 = 250.0;
const WINDOW_HEIGHT: f64 = 350.0;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app_state = AppState::new();

    if app_state.input.enable().is_err() {
        // TODO: display window asking for accessibility permissions
        // TODO: try to identify if the user accepted the permissions
        // because running `input_state.enable()` will open the permission
        // window again
        println!("Failed to enable input");
    }

    tauri::Builder::default()
        .manage(app_state)
        .on_window_event(window_events_handler)
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .invoke_handler(tauri::generate_handler![
            fetch_clipboard,
            paste,
            clear,
            quit,
            close,
            delete_item,
            get_shortcuts,
            save_shortcuts,
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let app_handle = app.handle().clone();

            let window_settings = window::Settings {
                width: WINDOW_WIDTH,
                height: WINDOW_HEIGHT,
                transparent: true,
                decorations: false,
            };

            register_shortcuts_plugin(&app_handle)?;
            load_and_register_shortcuts(&app_handle)?;
            tray::create(&app_handle)?;
            create_klipo_window(&app_handle, window_settings)
                .map_err(|_| tauri::Error::WindowNotFound)?;

            // TODO: implement shutdown
            let listener = ClipboardEventsListener::new(app_handle)?;
            std::thread::spawn(move || {
                listener.start().expect("Clipboard master shutdown");
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
