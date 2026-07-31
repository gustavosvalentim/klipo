use std::path::Path;
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
    pub fn new(database_path: impl AsRef<Path>) -> Result<Self, crate::clipboard::ClipboardError> {
        Ok(Self {
            clipboard: ClipboardStore::open(database_path)?,
            input: InputState::new(),
            focused_window_pid: Mutex::new(None),
            shortcuts: Mutex::new(ShortcutSettings::default()),
        })
    }
}
