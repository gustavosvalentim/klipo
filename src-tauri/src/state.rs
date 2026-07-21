use std::sync::Mutex;

use crate::clipboard::ClipboardStore;
use crate::input::InputState;
use crate::settings::ShortcutSettings;

pub struct AppState {
    pub clipboard: ClipboardStore,
    pub input: InputState,
    pub focused_window_pid: Mutex<Option<i32>>,
    pub shortcuts: Mutex<ShortcutSettings>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            clipboard: ClipboardStore::new(),
            input: InputState::new(),
            focused_window_pid: Mutex::new(None),
            shortcuts: Mutex::new(ShortcutSettings::default()),
        }
    }
}
