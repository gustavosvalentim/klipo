mod clipboard;
mod commands;
pub mod desktop;
mod input;
mod logging;
mod settings;
mod shortcuts;
mod state;
mod storage;
mod tray;
mod window;

use clipboard::ClipboardEventsListener;
use commands::{
    clear, close, delete_item, fetch_clipboard, get_shortcuts, log_frontend_error, paste, quit,
    save_shortcuts,
};
use log::{error, info};
use shortcuts::{load_and_register_shortcuts, register_shortcuts_plugin};
use state::AppState;
use tauri::Manager;
use window::{create_klipo_window, window_events_handler};

const WINDOW_WIDTH: f64 = 250.0;
const WINDOW_HEIGHT: f64 = 350.0;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .on_window_event(window_events_handler)
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            fetch_clipboard,
            log_frontend_error,
            paste,
            clear,
            quit,
            close,
            delete_item,
            get_shortcuts,
            save_shortcuts,
        ])
        .setup(|app| {
            // Logging is best-effort and must not prevent the app from starting.
            if let Ok(log_directory) = app.path().app_log_dir() {
                if logging::init(&log_directory).is_ok() {
                    info!(log_directory:debug = log_directory; "Application logging initialized");
                }
            }
            info!("Application starting");

            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let data_directory = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_directory)?;
            let app_state = AppState::new(data_directory.join("clipboard.sqlite3"))?;
            if let Err(error) = app_state.input.enable() {
                // TODO: display window asking for accessibility permissions
                error!(error:debug = error; "Failed to enable input");
            }
            app.manage(app_state);

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
            std::thread::spawn(move || listener.start());

            info!("Application started");
            Ok(())
        })
        .run(tauri::generate_context!())
        .unwrap_or_else(|error_value| {
            error!(error:debug = error_value; "Tauri application stopped with an error");
            std::process::exit(1);
        });
}
