use std::sync::Mutex;

use enigo::{Direction, Enigo, Key, Keyboard};
use tauri::Manager;
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::clipboard::{ClipboardItem, ClipboardStore};
use crate::window::get_main_window;

#[derive(Debug)]
pub enum PasteError {
    ClipboardError,
    ItemNotFound,
    WindowError,
    InputSimError(enigo::InputError),
}

impl std::fmt::Display for PasteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PasteError::ClipboardError => write!(f, "Clipboard error"),
            PasteError::ItemNotFound => write!(f, "Item not found"),
            PasteError::WindowError => write!(f, "Window error"),
            PasteError::InputSimError(e) => write!(f, "Input simulation error: {e}"),
        }
    }
}

pub fn paste(app: &tauri::AppHandle, text: &str) -> Result<ClipboardItem, PasteError> {
    let history = app.state::<ClipboardStore>();
    let paste_target = app.state::<PasteState>();
    let enigo = app.state::<Mutex<Enigo>>();

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

    if !paste_target.activate_last_focused_window() {
        return Err(PasteError::WindowError);
    }

    if let Ok(mut enigo) = enigo.lock() {
        let _ = simulate_paste_inputs(&mut enigo);
    }

    Ok(history.first().unwrap())
}

fn simulate_paste_inputs(enigo: &mut Enigo) -> Result<(), PasteError> {
    #[cfg(target_os = "macos")]
    let mod_key = Key::Meta;

    #[cfg(not(target_os = "macos"))]
    let mod_key = Key::Control;

    if let Err(e) = enigo.key(mod_key, Direction::Press) {
        return Err(PasteError::InputSimError(e));
    }

    if let Err(e) = enigo.key(Key::Unicode('v'), Direction::Press) {
        return Err(PasteError::InputSimError(e));
    } else {
        if let Err(e) = enigo.key(Key::Unicode('v'), Direction::Release) {
            return Err(PasteError::InputSimError(e));
        }
    }

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

    pub fn activate_last_focused_window(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            use crate::window::macos::set_focused_window;

            if let Ok(target) = self.last_focused_window.lock() {
                if let Some(pid) = target.pid {
                    set_focused_window(pid);
                    return true;
                }

                return false;
            }
        }

        false
    }
}
