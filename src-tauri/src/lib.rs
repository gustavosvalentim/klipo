mod clipboard;
mod commands;
mod input;
mod paste;
mod shortcuts;
mod tray;
mod window;

use clipboard::{ClipboardEventsListener, ClipboardStore};
use commands::{clear, close, delete_item, fetch_clipboard, paste, quit};
use input::InputState;
use paste::PasteState;
use shortcuts::register_shortcuts;
use window::{create_klipo_window, window_events_handler};

const WINDOW_WIDTH: f64 = 250.0;
const WINDOW_HEIGHT: f64 = 350.0;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let clipboard_store = ClipboardStore::new();
    let paste_target = PasteState::new();
    let input_state = InputState::new();

    if input_state.enable().is_err() {
        // TODO: display window asking for accessibility permissions
        println!("Failed to enable input");
    }

    tauri::Builder::default()
        .manage(clipboard_store)
        .manage(input_state)
        .manage(paste_target)
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

            if let Err(e) = create_klipo_window(&app_handle, window_settings) {
                panic!("Failed to create Klipo window: {e}");
            }

            if let Err(e) = tray::create(&app_handle) {
                panic!("Failed to create tray icon: {e}");
            }

            if let Err(e) = register_shortcuts(&app_handle) {
                panic!("Failed to register shortcuts: {e}");
            }

            // TODO: implement shutdown
            let listener = ClipboardEventsListener::new(app_handle);
            match listener {
                Ok(listener) => {
                    std::thread::spawn(move || {
                        listener.start().expect("Clipboard master shutdown");
                    });
                }
                Err(e) => {
                    panic!("Failed to create clipboard listener: {e}");
                }
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
