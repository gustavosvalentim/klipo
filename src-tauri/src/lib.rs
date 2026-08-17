mod clipboard;
mod commands;
pub mod desktop;
mod input;
mod logging;
mod settings;
mod shortcuts;
#[cfg(any(target_os = "linux", test))]
mod single_instance;
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
use shortcuts::{cleanup_global_shortcuts, initialize_global_shortcuts};
#[cfg(target_os = "linux")]
use single_instance::PickerActivation;
use state::AppState;
use tauri::Manager;
use window::{create_picker_window, window_events_handler};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(target_os = "linux")]
    let picker_activation = std::sync::Arc::new(PickerActivation::default());

    let builder = tauri::Builder::default();

    #[cfg(target_os = "linux")]
    let builder = single_instance::register(builder, std::sync::Arc::clone(&picker_activation));

    let application = builder
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
        .setup(move |app| {
            #[cfg(target_os = "linux")]
            let app_handle = app.handle().clone();

            #[cfg(all(
                target_os = "linux",
                debug_assertions,
                feature = "single-instance-test"
            ))]
            if single_instance::test_support::enabled() {
                return single_instance::run_primary_setup(
                    &picker_activation,
                    || single_instance::test_support::initialize_primary_resources(app),
                    || window::show_picker_window(&app_handle),
                    |error_value| {
                        error!(error:debug = error_value; "Failed to activate queued picker window");
                    },
                );
            }

            #[cfg(target_os = "linux")]
            {
                single_instance::run_primary_setup(
                    &picker_activation,
                    || initialize_application(app),
                    || window::show_picker_window(&app_handle),
                    |error_value| {
                        error!(error:debug = error_value; "Failed to activate queued picker window");
                    },
                )
            }

            #[cfg(not(target_os = "linux"))]
            initialize_application(app)
        })
        .build(tauri::generate_context!())
        .unwrap_or_else(|error_value| {
            error!(error:debug = error_value; "Tauri application could not start");
            std::process::exit(1);
        });

    application.run(|app_handle, event| {
        if matches!(event, tauri::RunEvent::ExitRequested { .. }) {
            if let Err(error_value) = cleanup_global_shortcuts(app_handle) {
                error!(error:% = error_value; "Failed to release global shortcut resources during shutdown");
            }
        }
    });
}

fn initialize_application(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
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

    initialize_global_shortcuts(&app_handle)?;
    tray::create(&app_handle)?;
    create_picker_window(&app_handle).map_err(|_| tauri::Error::WindowNotFound)?;

    // TODO: implement shutdown
    let listener = ClipboardEventsListener::new(app_handle)?;
    std::thread::spawn(move || listener.start());

    info!("Application started");
    Ok(())
}
