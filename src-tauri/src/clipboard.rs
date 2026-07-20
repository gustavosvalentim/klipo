use md5::{Digest, Md5};
use serde::Serialize;
use std::io;
use std::sync::{Arc, Mutex};
use std::vec::Vec;

use clipboard_master::{CallbackResult, ClipboardHandler, Master};
use tauri::{Emitter, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::state::AppState;
use crate::window::get_focused_window;

const MAX_ITEMS: usize = 120;

#[derive(Debug)]
pub enum ClipboardError {
    PoisonError,
    ItemNotFound,
}

impl std::fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClipboardError::PoisonError => write!(f, "Clipboard poisoned"),
            ClipboardError::ItemNotFound => write!(f, "Item not found"),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ClipboardStore {
    items: Mutex<Vec<ClipboardItem>>,
}

impl ClipboardStore {
    pub fn new() -> Self {
        Self {
            items: Mutex::new(Vec::new()),
        }
    }

    fn add_item(&self, item: ClipboardItem) {
        let mut history = self.items.lock().expect("Failed to lock clipboard history");

        if history.len() + 1 > MAX_ITEMS {
            history.pop();
        }

        history.insert(0, item);
    }

    fn hash(&self, text: &str) -> String {
        let text_digest = Md5::digest(text.as_bytes());
        format!("{:?}", text_digest)
    }

    pub fn add_text(&self, text: String) -> bool {
        if text.is_empty() {
            return false;
        }

        let hash = self.hash(&text);
        let item = ClipboardItem { text, hash };

        self.add_item(item);

        true
    }

    pub fn clear(&self) -> Result<(), ClipboardError> {
        self.items
            .lock()
            .map_err(|_| ClipboardError::PoisonError)?
            .clear();

        Ok(())
    }

    pub fn first(&self) -> Option<ClipboardItem> {
        let guard = self.items.lock().ok()?;

        if guard.is_empty() {
            None
        } else {
            Some(guard[0].clone())
        }
    }

    pub fn list(&self) -> Result<Vec<ClipboardItem>, ClipboardError> {
        let guard = self.items.lock().map_err(|_| ClipboardError::PoisonError)?;

        Ok(guard.clone())
    }

    pub fn exists(&self, text: &str) -> bool {
        let hash = self.hash(text);
        let Ok(guard) = self.items.lock() else {
            return false;
        };

        guard.iter().any(|item| item.hash == hash)
    }

    pub fn delete(&self, text: &str) -> Result<usize, ClipboardError> {
        let hash = self.hash(text);
        let mut history = match self.items.lock() {
            Ok(history) => history,
            Err(_) => return Err(ClipboardError::PoisonError),
        };

        let Some(idx) = history.iter().position(|item| item.hash == hash) else {
            return Err(ClipboardError::ItemNotFound);
        };

        history.remove(idx);

        Ok(idx)
    }

    pub fn move_to_top(&self, text: &str) -> Result<(), ClipboardError> {
        let hash = self.hash(text);

        let mut guard = self.items.lock().map_err(|_| ClipboardError::PoisonError)?;

        let item_idx = guard
            .iter()
            .position(|item| item.hash == hash)
            .ok_or(ClipboardError::ItemNotFound)?;

        let item = guard.remove(item_idx);

        guard.insert(0, item);

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ClipboardItem {
    pub text: String,
    pub hash: String,
}

pub struct ClipboardEventsListener {
    handler: Master<ClipboardEventsHandler>,
}

impl ClipboardEventsListener {
    pub fn new(app_handler: tauri::AppHandle) -> Result<ClipboardEventsListener, std::io::Error> {
        let handler = Master::new(ClipboardEventsHandler::new(Arc::new(app_handler)))?;
        Ok(Self { handler })
    }

    pub fn start(mut self) -> Result<(), std::io::Error> {
        self.handler.run()
    }
}

pub struct ClipboardEventsHandler {
    app: Arc<tauri::AppHandle>,
}

impl ClipboardEventsHandler {
    pub fn new(app: Arc<tauri::AppHandle>) -> Self {
        Self { app }
    }
}

impl ClipboardHandler for ClipboardEventsHandler {
    fn on_clipboard_change(&mut self) -> CallbackResult {
        println!("Clipboard changed");

        let klipo_pid = std::process::id();
        let focused_window_pid = get_focused_window();

        if let Some(focused_window_pid) = focused_window_pid {
            if focused_window_pid as u32 == klipo_pid {
                return CallbackResult::Next;
            }
        }

        let Ok(text) = self.app.clipboard().read_text() else {
            // TODO: add image support
            // this is probably an image and we should get it using
            // `AppHandle.clipboard().read_image()`.
            // Need to figure out how to handle this in the UI and backend
            return CallbackResult::Next;
        };

        let state = self.app.state::<AppState>();
        let store = &state.clipboard;

        if store.exists(&text) {
            if let Err(e) = store.move_to_top(&text) {
                println!("Failed to move item to top: {e}");
                return CallbackResult::Next;
            }

            return CallbackResult::Next;
        }

        if store.add_text(text) {
            let _ = self.app.emit_clipboard_changed();
        }

        CallbackResult::Next
    }

    fn on_clipboard_error(&mut self, error: io::Error) -> CallbackResult {
        println!("Clipboard error: {error}");
        CallbackResult::Next
    }
}

const CLIPBOARD_CHANGED_EVENT: &str = "clipboard-changed";

pub trait ClipboardEventsEmitter {
    fn emit_clipboard_changed(&self) -> Result<(), tauri::Error>;
}

impl ClipboardEventsEmitter for tauri::AppHandle {
    fn emit_clipboard_changed(&self) -> Result<(), tauri::Error> {
        self.emit(CLIPBOARD_CHANGED_EVENT, "")
    }
}
