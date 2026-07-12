use tauri::{Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, Window, WindowEvent};

const MAIN_WINDOW_LABEL: &str = "main";

pub struct Settings {
    pub width: f64,
    pub height: f64,
    pub transparent: bool,
    pub decorations: bool,
}

#[derive(Debug)]
pub enum WindowError {
    TauriError(tauri::Error),
}

impl std::fmt::Display for WindowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WindowError::TauriError(e) => write!(f, "Tauri error: {e}"),
        }
    }
}

pub fn create_clipbox_window(
    app: &tauri::AppHandle,
    settings: Settings,
) -> Result<WebviewWindow, WindowError> {
    let window = WebviewWindowBuilder::new(app, MAIN_WINDOW_LABEL, WebviewUrl::default())
        .inner_size(settings.width, settings.height)
        .decorations(settings.decorations)
        .transparent(settings.transparent)
        .always_on_top(true)
        .visible(false)
        .visible_on_all_workspaces(true)
        .shadow(false)
        .build();

    let window = match window {
        Ok(window) => window,
        Err(e) => return Err(WindowError::TauriError(e)),
    };

    Ok(window)
}

pub fn get_main_window(app: &tauri::AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(MAIN_WINDOW_LABEL)
}

pub fn window_events_handler(window: &Window, event: &WindowEvent) {
    if let WindowEvent::Focused(focused) = event {
        if !focused {
            let _ = window.hide();
        }
    }
}

#[cfg(target_os = "macos")]
pub mod macos {
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWorkspace};

    pub fn set_focused_window(pid: i32) {
        let app = NSRunningApplication::runningApplicationWithProcessIdentifier(pid);
        app.unwrap()
            .activateWithOptions(NSApplicationActivationOptions::empty());
    }

    pub fn active_window_pid() -> i32 {
        let workspace = NSWorkspace::sharedWorkspace();
        let app = workspace.frontmostApplication();

        app.unwrap().processIdentifier()
    }
}
