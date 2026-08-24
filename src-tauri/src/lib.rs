mod clipboard;
mod commands;
pub mod desktop;
mod input;
mod logging;
mod settings;
mod shortcuts;
#[cfg(any(target_os = "linux", test))]
mod single_instance;
mod startup;
mod state;
mod storage;
mod tray;
mod window;

use clipboard::{ClipboardEventsListener, SystemClipboard};
use commands::{
    clear, close, delete_item, fetch_clipboard, get_capabilities, get_shortcuts,
    log_frontend_error, paste, quit, save_shortcuts,
};
use desktop::{CapabilityUnavailableReason, DesktopCapability, DesktopSession};
use input::supports_input;
use log::{error, info, warn};
use shortcuts::{
    cleanup_global_shortcuts, load_shortcut_settings, register_loaded_shortcuts,
    register_shortcuts_plugin, supports_global_shortcuts,
};
#[cfg(target_os = "linux")]
use single_instance::PickerActivation;
use startup::{StartupCoordinator, StartupStep};
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
            get_capabilities,
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

    let session = desktop::detect_session();
    let mut startup = StartupCoordinator::new(session);
    let data_directory = app.path().app_data_dir()?;
    std::fs::create_dir_all(&data_directory)?;
    let mut app_state = AppState::new(data_directory.join("clipboard.sqlite3"), session)?;

    startup.run_steps(&StartupStep::ALL[..3], |step, startup| match step {
        StartupStep::Clipboard => initialize_system_clipboard(&mut app_state, startup, session),
        StartupStep::Input => initialize_input(&app_state, startup, session),
        StartupStep::TargetRestoration => set_target_restoration_capability(startup, session),
        StartupStep::Shortcut | StartupStep::Tray | StartupStep::Watcher | StartupStep::Window => {}
    });
    app_state.replace_capabilities(startup.capabilities());
    app.manage(app_state);

    let app_handle = app.handle().clone();

    startup.run_steps(&StartupStep::ALL[3..], |step, startup| match step {
        StartupStep::Shortcut => initialize_shortcuts(&app_handle, startup, session),
        StartupStep::Tray => initialize_tray(&app_handle, startup, session),
        StartupStep::Watcher => initialize_clipboard_watcher(&app_handle, startup, session),
        StartupStep::Window => initialize_window(&app_handle, startup, session),
        StartupStep::Clipboard | StartupStep::Input | StartupStep::TargetRestoration => {}
    });

    info!("Application started");
    Ok(())
}

fn initialize_system_clipboard(
    state: &mut AppState,
    startup: &mut StartupCoordinator,
    session: DesktopSession,
) {
    match startup.run_capability(
        &[
            DesktopCapability::ClipboardRead,
            DesktopCapability::ClipboardWrite,
        ],
        SystemClipboard::new,
        |_| CapabilityUnavailableReason::InitializationFailed,
    ) {
        Ok(system_clipboard) => state.install_system_clipboard(system_clipboard),
        Err(error_value) => log_startup_failure(
            session,
            DesktopCapability::ClipboardRead,
            CapabilityUnavailableReason::InitializationFailed,
            &error_value,
        ),
    }
}

fn initialize_input(state: &AppState, startup: &mut StartupCoordinator, session: DesktopSession) {
    let input_is_supported = supports_input(session);
    let input_result = startup.run_capability(
        &[DesktopCapability::Input, DesktopCapability::Pointer],
        || {
            if input_is_supported {
                state.input.enable().map_err(|error| format!("{error:?}"))
            } else {
                Err("input simulation is not supported by this desktop session".to_owned())
            }
        },
        |_| {
            if input_is_supported {
                CapabilityUnavailableReason::InitializationFailed
            } else {
                session_unavailable_reason(session)
            }
        },
    );

    if let Err(error_value) = input_result {
        let reason = if input_is_supported {
            CapabilityUnavailableReason::InitializationFailed
        } else {
            session_unavailable_reason(session)
        };
        log_startup_failure(session, DesktopCapability::Input, reason, &error_value);
    }
}

fn initialize_shortcuts(
    app: &tauri::AppHandle,
    startup: &mut StartupCoordinator,
    session: DesktopSession,
) {
    let state = app.state::<AppState>();

    if let Err(error_value) = load_shortcut_settings(app) {
        log_startup_failure(
            session,
            DesktopCapability::Shortcut,
            CapabilityUnavailableReason::InitializationFailed,
            &error_value,
        );
    }

    let result = startup.run_capability(
        &[DesktopCapability::Shortcut],
        || {
            if supports_global_shortcuts(session) {
                register_shortcuts_plugin(app)
                    .map_err(|error| {
                        (
                            CapabilityUnavailableReason::InitializationFailed,
                            error.to_string(),
                        )
                    })
                    .and_then(|_| {
                        register_loaded_shortcuts(app).map_err(|error| {
                            (CapabilityUnavailableReason::InitializationFailed, error)
                        })
                    })
            } else {
                Err((
                    session_unavailable_reason(session),
                    "global shortcuts are not supported by this session".to_owned(),
                ))
            }
        },
        |failure| failure.0,
    );

    if let Err((reason, error_value)) = result {
        log_startup_failure(session, DesktopCapability::Shortcut, reason, &error_value);
    }

    state.replace_capabilities(startup.capabilities());
}

fn initialize_tray(
    app: &tauri::AppHandle,
    startup: &mut StartupCoordinator,
    session: DesktopSession,
) {
    let state = app.state::<AppState>();

    let result = startup.run_capability(
        &[DesktopCapability::Tray],
        || tray::create(app),
        |_| CapabilityUnavailableReason::InitializationFailed,
    );

    if let Err(error_value) = result {
        log_startup_failure(
            session,
            DesktopCapability::Tray,
            CapabilityUnavailableReason::InitializationFailed,
            &error_value,
        );
    }

    state.replace_capabilities(startup.capabilities());
}

fn initialize_window(
    app: &tauri::AppHandle,
    startup: &mut StartupCoordinator,
    session: DesktopSession,
) {
    if let Err(error_value) = startup.run_shell_step(|| create_picker_window(app)) {
        warn!(
            session:? = session,
            backend = std::env::consts::OS,
            capability = "window",
            failure_category = "initialization_failed",
            error:debug = error_value;
            "Klipo window is unavailable; the shell will continue"
        );
    }
}

fn initialize_clipboard_watcher(
    app: &tauri::AppHandle,
    startup: &mut StartupCoordinator,
    session: DesktopSession,
) {
    let state = app.state::<AppState>();

    let result = startup.run_capability(
        &[DesktopCapability::Watcher],
        || {
            state
                .system_clipboard
                .as_ref()
                .ok_or_else(|| "system clipboard initialization failed".to_owned())?;
            ClipboardEventsListener::new(app.clone()).map_err(|error| error.to_string())
        },
        |_| CapabilityUnavailableReason::InitializationFailed,
    );

    match result {
        Ok(listener) => {
            std::thread::spawn(move || listener.start());
        }
        Err(error_value) => log_startup_failure(
            session,
            DesktopCapability::Watcher,
            CapabilityUnavailableReason::InitializationFailed,
            &error_value,
        ),
    }

    state.replace_capabilities(startup.capabilities());
}

fn set_target_restoration_capability(startup: &mut StartupCoordinator, session: DesktopSession) {
    #[cfg(target_os = "macos")]
    {
        let _ = session;
        let _ = startup.run_capability(
            &[DesktopCapability::TargetRestoration],
            || Ok::<(), CapabilityUnavailableReason>(()),
            |reason| *reason,
        );
    }

    #[cfg(target_os = "linux")]
    {
        let restoration_supported =
            session == DesktopSession::X11 && window::supports_target_restoration();
        let result = startup.run_capability(
            &[DesktopCapability::TargetRestoration],
            || target_restoration_result(session, restoration_supported),
            |reason| *reason,
        );
        if let Err(error_value) = result {
            log_startup_failure(
                session,
                DesktopCapability::TargetRestoration,
                error_value,
                &"target restoration is unavailable for this desktop session",
            );
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let reason = target_restoration_unavailable_reason(session);
        let result = startup.run_capability(
            &[DesktopCapability::TargetRestoration],
            || Err::<(), _>(reason),
            |reason| *reason,
        );
        if let Err(error_value) = result {
            log_startup_failure(
                session,
                DesktopCapability::TargetRestoration,
                error_value,
                &"target restoration is not implemented for this platform",
            );
        }
    }
}

fn session_unavailable_reason(session: DesktopSession) -> CapabilityUnavailableReason {
    match session {
        DesktopSession::Unknown => CapabilityUnavailableReason::UnknownSession,
        DesktopSession::X11 | DesktopSession::Wayland => {
            CapabilityUnavailableReason::UnsupportedSession
        }
    }
}

#[cfg(any(test, not(target_os = "macos")))]
fn target_restoration_unavailable_reason(session: DesktopSession) -> CapabilityUnavailableReason {
    match session {
        DesktopSession::X11 => CapabilityUnavailableReason::AdapterUnavailable,
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
