#[cfg(target_os = "macos")]
use tauri::window::{Effect, EffectState, EffectsBuilder};
use tauri::{Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, Window, WindowEvent};

use crate::state::AppState;

const MAIN_WINDOW_LABEL: &str = "main";
const SETTINGS_WINDOW_LABEL: &str = "settings";

pub struct Settings {
    pub width: f64,
    pub height: f64,
    pub transparent: bool,
    pub decorations: bool,
}

pub(crate) const PICKER_WIDTH: f64 = 250.0;
pub(crate) const PICKER_HEIGHT: f64 = 350.0;

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

pub fn create_klipo_window(
    app: &tauri::AppHandle,
    settings: Settings,
) -> Result<WebviewWindow, WindowError> {
    let window_builder = WebviewWindowBuilder::new(app, MAIN_WINDOW_LABEL, WebviewUrl::default())
        .inner_size(settings.width, settings.height)
        .decorations(settings.decorations)
        .transparent(settings.transparent)
        .always_on_top(true)
        .visible(false)
        .visible_on_all_workspaces(true)
        .shadow(false);

    #[cfg(target_os = "macos")]
    let window_builder = window_builder
        // `Menu` matches a macOS popup menu more closely than `Popover`.
        // Its radius clips the native backdrop too, rather than leaving a
        // square vibrancy layer behind the rounded web content.
        .effects(
            EffectsBuilder::new()
                .effect(Effect::Menu)
                .state(EffectState::Active)
                .radius(11.0)
                .build(),
        );

    let window = window_builder.build();

    let window = match window {
        Ok(window) => window,
        Err(e) => return Err(WindowError::TauriError(e)),
    };

    Ok(window)
}

pub fn create_picker_window(app: &tauri::AppHandle) -> Result<WebviewWindow, WindowError> {
    create_klipo_window(
        app,
        Settings {
            width: PICKER_WIDTH,
            height: PICKER_HEIGHT,
            transparent: cfg!(target_os = "macos"),
            decorations: false,
        },
    )
}

pub fn get_main_window(app: &tauri::AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(MAIN_WINDOW_LABEL)
}

#[cfg(target_os = "linux")]
pub fn show_picker_window(app: &tauri::AppHandle) -> Result<(), tauri::Error> {
    let window = get_main_window(app).ok_or(tauri::Error::WindowNotFound)?;

    window.show()?;
    window.set_focus()
}

pub fn show_settings_window(app: &tauri::AppHandle) -> Result<(), tauri::Error> {
    let window = match app.get_webview_window(SETTINGS_WINDOW_LABEL) {
        Some(window) => window,
        None => WebviewWindowBuilder::new(app, SETTINGS_WINDOW_LABEL, WebviewUrl::default())
            .title("Klipo Settings")
            .inner_size(560.0, 510.0)
            .min_inner_size(560.0, 510.0)
            .resizable(false)
            .build()?,
    };
    window.show()?;
    window.set_focus()
}

pub fn window_events_handler(window: &Window, event: &WindowEvent) {
    if window.label() == SETTINGS_WINDOW_LABEL {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = window.hide();
        }
        return;
    }
    if let WindowEvent::Focused(focused) = event {
        if !focused {
            let _ = window.hide();
        }
    }
}

#[derive(Debug)]
pub enum FocusError {
    PlatformUnsupported,
    StatePoisoned,
    FocusedWindowUnavailable,
}

/// Opaque platform focus target. Native process identifiers remain private to
/// this module so callers can only capture, restore, or compare focus.
#[derive(Default)]
pub(crate) struct FocusTarget {
    process_id: Option<i32>,
}

impl FocusTarget {
    pub(crate) fn empty() -> Self {
        Self::default()
    }

    fn current() -> Self {
        Self {
            process_id: focused_process_id(),
        }
    }

    fn restore(&self) -> Result<(), FocusError> {
        self.restore_with(activate_process)
    }

    fn restore_with(&self, activate: impl FnOnce(i32) -> bool) -> Result<(), FocusError> {
        let process_id = self
            .process_id
            .ok_or(FocusError::FocusedWindowUnavailable)?;

        if activate(process_id) {
            Ok(())
        } else {
            Err(FocusError::PlatformUnsupported)
        }
    }
}

impl std::fmt::Display for FocusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FocusError::PlatformUnsupported => write!(f, "Platform unsupported"),
            FocusError::StatePoisoned => write!(f, "Focused window state poisoned"),
            FocusError::FocusedWindowUnavailable => write!(f, "Focused window unavailable"),
        }
    }
}

pub fn capture_focused_window(state: &AppState) -> Result<(), FocusError> {
    let mut focus_target = state
        .focus_target
        .lock()
        .map_err(|_| FocusError::StatePoisoned)?;

    *focus_target = FocusTarget::current();

    Ok(())
}

pub fn restore_focused_window(state: &AppState) -> Result<(), FocusError> {
    let focus_target = state
        .focus_target
        .lock()
        .map_err(|_| FocusError::StatePoisoned)?;

    focus_target.restore()
}

pub fn is_klipo_focused() -> bool {
    is_current_process(focused_process_id(), std::process::id())
}

fn is_current_process(focused_process_id: Option<i32>, current_process_id: u32) -> bool {
    focused_process_id == i32::try_from(current_process_id).ok()
}

fn focused_process_id() -> Option<i32> {
    #[cfg(target_os = "macos")]
    {
        use crate::window::macos::focused_process_id;

        focused_process_id()
    }

    #[cfg(not(target_os = "macos"))]
    {
        log::warn!(platform = std::env::consts::OS; "Focused window lookup is not implemented");
        None
    }
}

fn activate_process(process_id: i32) -> bool {
    #[cfg(target_os = "macos")]
    {
        use crate::window::macos::activate_process;

        activate_process(process_id)
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = process_id;
        log::warn!(platform = std::env::consts::OS; "Focused window restoration is not implemented");
        false
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWorkspace};

    pub fn activate_process(pid: i32) -> bool {
        let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) else {
            return false;
        };

        app.activateWithOptions(NSApplicationActivationOptions::empty())
    }

    pub fn focused_process_id() -> Option<i32> {
        let workspace = NSWorkspace::sharedWorkspace();
        let app = workspace.frontmostApplication();

        Some(app?.processIdentifier())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_klipo_as_the_focused_process_without_exposing_its_identifier() {
        assert!(is_current_process(Some(42), 42));
        assert!(!is_current_process(Some(41), 42));
        assert!(!is_current_process(None, 42));
    }

    #[test]
    fn restores_the_opaque_target_only_inside_the_platform_boundary() {
        let target = FocusTarget {
            process_id: Some(42),
        };
        let mut activated = None;

        let result = target.restore_with(|process_id| {
            activated = Some(process_id);
            true
        });

        assert!(result.is_ok());
        assert_eq!(activated, Some(42));
    }

    #[test]
    fn does_not_restore_when_no_focus_target_was_captured() {
        let result = FocusTarget::empty().restore_with(|_| true);

        assert!(matches!(result, Err(FocusError::FocusedWindowUnavailable)));
    }
}
