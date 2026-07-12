use std::sync::{Mutex, MutexGuard};

use enigo::{Direction, Enigo, Key, Keyboard};
use tauri::Manager;
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::clipboard::{ClipboardManager, InMemoryClipboardHistory};
use crate::window::get_main_window;

#[derive(Debug)]
pub enum PasteError {
    ClipboardError,
    ItemNotFound,
    WindowError,
}

impl std::fmt::Display for PasteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PasteError::ClipboardError => write!(f, "Clipboard error"),
            PasteError::ItemNotFound => write!(f, "Item not found"),
            PasteError::WindowError => write!(f, "Window error"),
        }
    }
}

pub fn paste(app: &tauri::AppHandle, text: &str) -> Result<(), PasteError> {
    let history = app.state::<InMemoryClipboardHistory>();
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

    if let Some(target) = paste_target.target() {
        target.activate();
    }

    if let Ok(mut enigo) = enigo.lock() {
        platform_paste(&mut enigo);
    }

    Ok(())
}

fn platform_paste(enigo: &mut Enigo) {
    #[cfg(target_os = "macos")]
    let mod_key = Key::Meta;

    #[cfg(not(target_os = "macos"))]
    let mod_key = Key::Control;

    let _ = enigo.key(mod_key, Direction::Press);
    let _ = enigo.key(Key::Unicode('v'), Direction::Press);
    let _ = enigo.key(mod_key, Direction::Release);
}

pub struct PasteState {
    target: Mutex<PlatformPasteTarget>,
}

impl PasteState {
    pub fn new() -> Self {
        Self {
            target: Mutex::new(PlatformPasteTarget::new().unwrap()),
        }
    }

    pub fn target(&self) -> Option<MutexGuard<'_, PlatformPasteTarget>> {
        self.target.lock().ok()
    }
}

pub enum PlatformPasteTarget {
    #[cfg(target_os = "macos")]
    MacOS(macos::MacOSPasteTarget),
    #[cfg(target_os = "windows")]
    Windows,
    #[cfg(target_os = "linux")]
    Linux,
}

impl PlatformPasteTarget {
    pub fn new() -> Option<Self> {
        #[cfg(target_os = "macos")]
        {
            Some(PlatformPasteTarget::MacOS(macos::MacOSPasteTarget::new()))
        }

        #[cfg(target_os = "windows")]
        {
            Some(PlatformPasteTarget::Windows)
        }

        #[cfg(target_os = "linux")]
        {
            Some(PlatformPasteTarget::Linux)
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            None
        }
    }

    pub fn get_current_target(&mut self) {
        match self {
            #[cfg(target_os = "macos")]
            PlatformPasteTarget::MacOS(target) => target.get_current_target(),
            #[cfg(target_os = "windows")]
            PlatformPasteTarget::Windows => Some(PlatformPasteTarget::Windows),
            #[cfg(target_os = "linux")]
            PlatformPasteTarget::Linux => Some(PlatformPasteTarget::Linux),
        }
    }

    pub fn activate(&self) {
        match self {
            PlatformPasteTarget::MacOS(target) => target.activate(),
            #[cfg(target_os = "windows")]
            PlatformPasteTarget::Windows => {}
            #[cfg(target_os = "linux")]
            PlatformPasteTarget::Linux => {}
        }
    }
}

pub trait PasteTarget {
    fn get_current_target(&mut self);
    fn activate(&self);
}

#[cfg(target_os = "macos")]
pub mod macos {
    use super::PasteTarget;
    use crate::window::macos::{active_window_pid, set_focused_window};

    pub struct MacOSPasteTarget {
        app: i32,
    }

    impl MacOSPasteTarget {
        pub fn new() -> Self {
            MacOSPasteTarget { app: 0 }
        }
    }

    impl PasteTarget for MacOSPasteTarget {
        fn get_current_target(&mut self) {
            self.app = active_window_pid();
        }

        fn activate(&self) {
            set_focused_window(self.app);
        }
    }
}
