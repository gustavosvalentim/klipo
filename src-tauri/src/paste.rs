use std::sync::Mutex;

use enigo::{Direction, Enigo, Key, Keyboard};
use tauri::Manager;
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::clipboard::ClipboardStore;
use crate::input::InputState;
use crate::window::get_main_window;

#[derive(Debug)]
pub enum PasteError {
    ClipboardError,
    ItemNotFound,
    WindowError,
    InputSimError(enigo::InputError),
    PermissionError,
}

impl std::fmt::Display for PasteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PasteError::ClipboardError => write!(f, "Clipboard error"),
            PasteError::ItemNotFound => write!(f, "Item not found"),
            PasteError::WindowError => write!(f, "Window error"),
            PasteError::InputSimError(e) => write!(f, "Input simulation error: {e}"),
            PasteError::PermissionError => write!(f, "Permission error"),
        }
    }
}

pub fn paste_from_selection(app: &tauri::AppHandle, text: &str) -> Result<(), PasteError> {
    let history = app.state::<ClipboardStore>();
    let paste_target = app.state::<PasteState>();
    let input_state = app.state::<InputState>();

    if !history.exists(text) {
        return Err(PasteError::ItemNotFound);
    }

    if app.clipboard().write_text(text).is_err() {
        return Err(PasteError::ClipboardError);
    }

    let _ = history.move_to_top(text);

    if let Some(window) = get_main_window(app) {
        if window.hide().is_err() {
            return Err(PasteError::WindowError);
        }
    }

    if paste_target.restore_focus().is_err() {
        return Err(PasteError::WindowError);
    }

    if let Ok(mut input) = input_state.enigo.lock() {
        let Some(ref mut enigo) = *input else {
            return Err(PasteError::PermissionError);
        };

        let _ = simulate_paste_inputs(enigo);
    }

    Ok(())
}

fn simulate_paste_inputs(enigo: &mut Enigo) -> Result<(), PasteError> {
    #[cfg(target_os = "macos")]
    let mod_key = Key::Meta;

    #[cfg(not(target_os = "macos"))]
    let mod_key = Key::Control;

    if let Err(e) = enigo.key(mod_key, Direction::Press) {
        return Err(PasteError::InputSimError(e));
    }

    let _ = enigo.key(Key::Unicode('v'), Direction::Click);

    if let Err(e) = enigo.key(mod_key, Direction::Release) {
        return Err(PasteError::InputSimError(e));
    }

    Ok(())
}

pub struct AppInfo {
    pub pid: Option<i32>,
}

pub struct PasteState {
    last_focused_window: Mutex<AppInfo>,
}

pub enum PasteStateError {
    PlatformUnsupported,
    StatePoisonError,
    WindowHandlerError,
}

impl PasteState {
    pub fn new() -> Self {
        Self {
            last_focused_window: Mutex::new(AppInfo { pid: None }),
        }
    }

    pub fn load_focused_window(&self) {
        #[cfg(target_os = "macos")]
        {
            use crate::window::macos::active_window_pid;

            if let Ok(mut target) = self.last_focused_window.lock() {
                target.pid = active_window_pid();
            }
        }
    }

    pub fn restore_focus(&self) -> Result<(), PasteStateError> {
        #[cfg(target_os = "macos")]
        {
            use crate::window::macos::set_focused_window;

            if let Ok(target) = self.last_focused_window.lock() {
                let Some(pid) = target.pid else {
                    return Err(PasteStateError::WindowHandlerError);
                };

                if set_focused_window(pid) {
                    return Ok(());
                }

                return Err(PasteStateError::StatePoisonError);
            }
        }

        Err(PasteStateError::PlatformUnsupported)
    }
}
