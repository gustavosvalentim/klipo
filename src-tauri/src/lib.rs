mod clipboard;
mod commands;
mod content;
pub mod desktop;
mod input;
mod logging;
mod picker;
mod settings;
mod shortcuts;
#[cfg(any(target_os = "linux", test))]
mod single_instance;
mod state;
mod storage;
mod tray;
mod window;

use clipboard::{ClipboardEventsListener, SystemClipboard};
use commands::{
    clear, close, close_settings, delete_item, fetch_clipboard, get_capabilities, get_shortcuts,
    log_frontend_error, paste, quit, save_shortcuts, show_settings,
};
use desktop::{CapabilityStatus, CapabilityUnavailableReason, DesktopCapability, DesktopSession};
use input::supports_input;
use log::{error, info, warn};
use shortcuts::{
    cleanup_global_shortcuts, load_shortcut_settings, register_loaded_shortcuts,
    register_shortcuts_plugin, supports_global_shortcuts,
};
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
        .invoke_handler(tauri::generate_handler![
            fetch_clipboard,
            get_capabilities,
            log_frontend_error,
            paste,
            clear,
            quit,
            show_settings,
            close,
            close_settings,
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
                    || picker::open(&app_handle),
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
                    || picker::open(&app_handle),
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
            if let Some(app_state) = app_handle.try_state::<AppState>() {
                app_state.shutdown_clipboard_watcher();
            }
            if let Err(error_value) = cleanup_global_shortcuts(app_handle) {
                error!(error:% = error_value; "Failed to release global shortcut resources during shutdown");
            }

            tray::remove(app_handle);
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

    let session = desktop::detect_session();
    let data_directory = app.path().app_data_dir()?;
    std::fs::create_dir_all(&data_directory)?;
    let mut app_state = AppState::new(data_directory.join("clipboard.sqlite3"), session)?;

    initialize_system_clipboard(&mut app_state, session);
    initialize_input(&app_state, session);
    initialize_pointer(app, &app_state, session);
    set_target_restoration_capability(&app_state, session);
    app.manage(app_state);

    let app_handle = app.handle().clone();
    initialize_shortcuts(&app_handle, session);
    initialize_tray(&app_handle, session);
    initialize_clipboard_watcher(&app_handle, session);
    initialize_window(&app_handle, session);

    info!("Application started");
    Ok(())
}

fn record_capability(
    state: &AppState,
    capability: DesktopCapability,
    reason: Option<CapabilityUnavailableReason>,
) {
    let status = match reason {
        Some(reason) => CapabilityStatus::unavailable(reason),
        None => CapabilityStatus::available(),
    };
    state.set_capability(capability, status);
}

fn initialize_system_clipboard(state: &mut AppState, session: DesktopSession) {
    match SystemClipboard::new() {
        Ok(system_clipboard) => {
            state.install_system_clipboard(system_clipboard);
        }
        Err(error_value) => {
            for capability in [
                DesktopCapability::ClipboardRead,
                DesktopCapability::ClipboardWrite,
            ] {
                record_capability(
                    state,
                    capability,
                    Some(CapabilityUnavailableReason::InitializationFailed),
                );
            }
            log_startup_failure(
                session,
                DesktopCapability::ClipboardRead,
                CapabilityUnavailableReason::InitializationFailed,
                &error_value,
            );
        }
    }
}

fn initialize_input(state: &AppState, session: DesktopSession) {
    let supported = supports_input(session);
    let initialization = if supported {
        state
            .input
            .enable()
            .map_err(|error_value| format!("{error_value:?}"))
    } else {
        Err("input simulation is not supported by this desktop session".to_owned())
    };
    let reason = if supported {
        CapabilityUnavailableReason::InitializationFailed
    } else {
        session_unavailable_reason(session)
    };
    record_capability(
        state,
        DesktopCapability::Input,
        initialization.as_ref().err().map(|_| reason),
    );
    if let Err(error_value) = initialization {
        log_startup_failure(session, DesktopCapability::Input, reason, &error_value);
    }
}

fn initialize_pointer(app: &tauri::App, state: &AppState, session: DesktopSession) {
    let availability = app.handle().cursor_position();
    record_capability(
        state,
        DesktopCapability::Pointer,
        availability
            .as_ref()
            .err()
            .map(|_| CapabilityUnavailableReason::InitializationFailed),
    );
    if let Err(error_value) = availability {
        log_startup_failure(
            session,
            DesktopCapability::Pointer,
            CapabilityUnavailableReason::InitializationFailed,
            &error_value,
        );
    }
}

fn initialize_shortcuts(app: &tauri::AppHandle, session: DesktopSession) {
    let state = app.state::<AppState>();
    if let Err(error_value) = load_shortcut_settings(app) {
        log_startup_failure(
            session,
            DesktopCapability::Shortcut,
            CapabilityUnavailableReason::InitializationFailed,
            &error_value,
        );
    }
    let initialization = if supports_global_shortcuts(session) {
        register_shortcuts_plugin(app)
            .map_err(|error_value| error_value.to_string())
            .and_then(|_| register_loaded_shortcuts(app))
    } else {
        Err("global shortcuts are not supported by this session".to_owned())
    };
    let reason = if supports_global_shortcuts(session) {
        CapabilityUnavailableReason::InitializationFailed
    } else {
        session_unavailable_reason(session)
    };
    record_capability(
        &state,
        DesktopCapability::Shortcut,
        initialization.as_ref().err().map(|_| reason),
    );
    if let Err(error_value) = initialization {
        log_startup_failure(session, DesktopCapability::Shortcut, reason, &error_value);
    }
}

fn initialize_tray(app: &tauri::AppHandle, session: DesktopSession) {
    let state = app.state::<AppState>();
    let initialization = tray::create(app).map(|tray_icon| {
        let _ = app.manage(tray::RetainedTrayIcon::new(tray_icon));
    });
    record_capability(
        &state,
        DesktopCapability::Tray,
        initialization
            .as_ref()
            .err()
            .map(|_| CapabilityUnavailableReason::InitializationFailed),
    );
    if let Err(error_value) = initialization {
        log_startup_failure(
            session,
            DesktopCapability::Tray,
            CapabilityUnavailableReason::InitializationFailed,
            &error_value,
        );
    }
}

fn initialize_window(app: &tauri::AppHandle, session: DesktopSession) {
    if let Err(error_value) = create_picker_window(app) {
        warn!(session:? = session, backend = std::env::consts::OS, capability = "window", failure_category = "initialization_failed", error:debug = error_value; "Klipo window is unavailable; the shell will continue");
    }
}

fn initialize_clipboard_watcher(app: &tauri::AppHandle, session: DesktopSession) {
    let state = app.state::<AppState>();
    let initialization = if state.system_clipboard.is_some() {
        ClipboardEventsListener::new(app.clone()).map_err(|error_value| error_value.to_string())
    } else {
        Err("system clipboard initialization failed".to_owned())
    };
    record_capability(
        &state,
        DesktopCapability::Watcher,
        initialization
            .as_ref()
            .err()
            .map(|_| CapabilityUnavailableReason::InitializationFailed),
    );
    match initialization {
        Ok((listener, watcher_shutdown)) => {
            state.install_clipboard_watcher(watcher_shutdown);
            std::thread::spawn(move || listener.start());
        }
        Err(error_value) => log_startup_failure(
            session,
            DesktopCapability::Watcher,
            CapabilityUnavailableReason::InitializationFailed,
            &error_value,
        ),
    }
}

fn set_target_restoration_capability(state: &AppState, session: DesktopSession) {
    #[cfg(target_os = "macos")]
    let reason = {
        let _ = session;
        None
    };
    #[cfg(target_os = "linux")]
    let reason = target_restoration_result(
        session,
        session == DesktopSession::X11 && window::supports_target_restoration(),
    )
    .err();
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let reason = Some(target_restoration_unavailable_reason(session));
    record_capability(state, DesktopCapability::TargetRestoration, reason);
    if let Some(reason) = reason {
        log_startup_failure(
            session,
            DesktopCapability::TargetRestoration,
            reason,
            &"target restoration is unavailable for this desktop session",
        );
    }
}

fn session_unavailable_reason(session: DesktopSession) -> CapabilityUnavailableReason {
    match session {
        DesktopSession::Unknown => CapabilityUnavailableReason::UnknownSession,
        DesktopSession::Macos | DesktopSession::X11 | DesktopSession::Wayland => {
            CapabilityUnavailableReason::UnsupportedSession
        }
    }
}

#[cfg(any(test, not(target_os = "macos")))]
fn target_restoration_unavailable_reason(session: DesktopSession) -> CapabilityUnavailableReason {
    match session {
        DesktopSession::Macos | DesktopSession::X11 => {
            CapabilityUnavailableReason::AdapterUnavailable
        }
        DesktopSession::Wayland => CapabilityUnavailableReason::UnsupportedSession,
        DesktopSession::Unknown => CapabilityUnavailableReason::UnknownSession,
    }
}

#[cfg(any(target_os = "linux", test))]
fn target_restoration_result(
    session: DesktopSession,
    restoration_supported: bool,
) -> Result<(), CapabilityUnavailableReason> {
    if session != DesktopSession::X11 {
        Err(target_restoration_unavailable_reason(session))
    } else if restoration_supported {
        Ok(())
    } else {
        Err(CapabilityUnavailableReason::AdapterUnavailable)
    }
}

fn log_startup_failure(
    session: DesktopSession,
    capability: DesktopCapability,
    reason: CapabilityUnavailableReason,
    error_value: &impl std::fmt::Debug,
) {
    warn!(
        session:? = session,
        backend = std::env::consts::OS,
        capability:? = capability,
        failure_category:? = reason,
        error:debug = error_value;
        "Desktop integration unavailable; continuing startup"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_target_restoration_failures_to_session_specific_reasons() {
        assert_eq!(
            target_restoration_unavailable_reason(DesktopSession::X11),
            CapabilityUnavailableReason::AdapterUnavailable
        );
        assert_eq!(
            target_restoration_unavailable_reason(DesktopSession::Wayland),
            CapabilityUnavailableReason::UnsupportedSession
        );
        assert_eq!(
            target_restoration_unavailable_reason(DesktopSession::Unknown),
            CapabilityUnavailableReason::UnknownSession
        );
    }

    #[test]
    fn enables_target_restoration_only_for_a_probed_x11_adapter() {
        assert_eq!(target_restoration_result(DesktopSession::X11, true), Ok(()));
        assert_eq!(
            target_restoration_result(DesktopSession::X11, false),
            Err(CapabilityUnavailableReason::AdapterUnavailable)
        );
        assert_eq!(
            target_restoration_result(DesktopSession::Wayland, true),
            Err(CapabilityUnavailableReason::UnsupportedSession)
        );
    }
}
