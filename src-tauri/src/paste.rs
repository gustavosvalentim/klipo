use std::sync::Mutex;

use enigo::{Direction, Enigo, Key, Keyboard};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::clipboard::ClipboardStore;
use crate::input::InputState;
use crate::window::get_main_window;

#[derive(Debug)]
pub enum PasteError {
    ClipboardError,
    InputSimError(enigo::InputError),
    ItemNotFound,
    PoisonError,
    PermissionError,
    WindowError,
}

impl std::fmt::Display for PasteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PasteError::ClipboardError => write!(f, "Clipboard error"),
            PasteError::InputSimError(e) => write!(f, "Input simulation error: {e}"),
            PasteError::ItemNotFound => write!(f, "Item not found"),
            PasteError::PermissionError => write!(f, "Permission error"),
            PasteError::PoisonError => write!(f, "Poison error"),
            PasteError::WindowError => write!(f, "Window error"),
        }
    }
}

pub fn paste_from_selection(app: &tauri::AppHandle, history: &ClipboardStore, paste_target: &WindowManager, input_state: &InputState, text: &str) -> Result<(), PasteError> {
    if !history.exists(text) {
        return Err(PasteError::ItemNotFound);
    }

    app.clipboard().write_text(text).map_err(|_| PasteError::ClipboardError)?;

    let _ = history.move_to_top(text);

    if let Some(window) = get_main_window(app) {
        window.hide().map_err(|_| PasteError::WindowError)?;
    }

    paste_target.restore_focus().map_err(|_| PasteError::WindowError)?;

    let mut guard = input_state.enigo.lock().map_err(|_| PasteError::PoisonError)?;
    let enigo = guard
        .as_mut()
        .ok_or(PasteError::PermissionError)?;

    simulate_paste_inputs(enigo)
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

pub struct WindowManager {
    last_focused_window: Mutex<AppInfo>,
}

#[derive(Debug)]
pub enum PasteStateError {
    PlatformUnsupported,
    StatePoisonError,
    WindowHandlerError,
}

impl std::fmt::Display for PasteStateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PasteStateError::PlatformUnsupported => write!(f, "Platform unsupported"),
            PasteStateError::StatePoisonError => write!(f, "State poison error"),
            PasteStateError::WindowHandlerError => write!(f, "Window handler error"),
        }
    }
}

impl WindowManager {
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

        #[cfg(not(target_os = "macos"))]
        {
            println!("Not implemented");
        }
    }

    pub fn restore_focus(&self) -> Result<(), PasteStateError> {
        #[cfg(target_os = "macos")]
        {
            use crate::window::macos::set_focused_window;

            let target = self.last_focused_window.lock().map_err(|_| PasteStateError::StatePoisonError)?;
            let pid = target.pid.ok_or(PasteStateError::WindowHandlerError)?;

            if set_focused_window(pid) {
                Ok(())
            } else {
                Err(PasteStateError::StatePoisonError)
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            Err(PasteStateError::PlatformUnsupported)
        }
    }
}
