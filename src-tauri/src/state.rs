use std::path::Path;
use std::sync::Mutex;

use crate::clipboard::{SystemClipboard, SystemClipboardError};
use crate::input::InputState;
use crate::settings::ShortcutSettings;
use crate::storage::{ClipboardError, ClipboardStore};

#[derive(Debug)]
pub enum AppStateError {
    Clipboard(ClipboardError),
    SystemClipboard(SystemClipboardError),
}

impl std::fmt::Display for AppStateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Clipboard(error) => error.fmt(f),
            Self::SystemClipboard(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for AppStateError {}

impl From<ClipboardError> for AppStateError {
    fn from(error: ClipboardError) -> Self {
        Self::Clipboard(error)
    }
}

impl From<SystemClipboardError> for AppStateError {
    fn from(error: SystemClipboardError) -> Self {
        Self::SystemClipboard(error)
    }
}

pub struct AppState {
    pub clipboard: ClipboardStore,
    pub system_clipboard: SystemClipboard,
    pub input: InputState,
    pub focused_window_pid: Mutex<Option<i32>>,
    pub shortcuts: Mutex<ShortcutSettings>,
}

impl AppState {
    pub fn new(database_path: impl AsRef<Path>) -> Result<Self, AppStateError> {
        Ok(Self {
            clipboard: ClipboardStore::open(database_path)?,
            system_clipboard: SystemClipboard::new()?,
            input: InputState::new(),
            focused_window_pid: Mutex::new(None),
            shortcuts: Mutex::new(ShortcutSettings::default()),
        })
    }
}
