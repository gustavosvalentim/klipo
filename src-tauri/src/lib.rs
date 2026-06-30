mod clipboard;
mod commands;
mod paste;
mod shortcuts;
mod tray;
mod window;

use clipboard::{InMemoryClipboardHistory, ClipboardManager};
use commands::{clear_clipboard_items, list_clipboard_items, paste_from_selection};
use window::{window_events_handler};
use shortcuts::register_shortcuts;

use std::sync::{Arc, Mutex};

use enigo::{Enigo, Settings};

const WINDOW_WIDTH: f64 = 200.0;
const WINDOW_HEIGHT: f64 = 300.0;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let clipboard_history = InMemoryClipboardHistory::new_manager();

    let enigo = match Enigo::new(&Settings::default()) {
        Ok(enigo) => Arc::new(Mutex::new(enigo)),
        Err(e) => {
            panic!("Failed to create Enigo instance: {e}");
        }
    };

    tauri::Builder::default()
        .manage(clipboard_history.clone())
        .manage(enigo.clone())
        .on_window_event(window_events_handler)
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .invoke_handler(tauri::generate_handler![
            list_clipboard_items,
            paste_from_selection,
            clear_clipboard_items
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
                radius: 12.0,
            };
            if let Err(e) = window::create(&app_handle, window_settings) {
                panic!("Failed to create window: {e}");
            }

            if let Err(e) = tray::create(&app_handle) {
                panic!("Failed to create tray icon: {e}");
            }

            if let Err(e) = register_shortcuts(&app_handle) {
                panic!("Failed to register shortcuts: {e}");
            }

            let mut listener = clipboard::change_listener(app_handle, clipboard_history);

            tauri::async_runtime::spawn(async move {
                listener.run().expect("Clipboard master shutdown");
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
