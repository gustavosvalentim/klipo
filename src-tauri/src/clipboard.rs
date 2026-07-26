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

    fn hash_text(text: &str) -> String {
        let mut hasher = Md5::new();
        hasher.update(b"text:");
        hasher.update(text.as_bytes());
        let digest = hasher.finalize();
        let hex = digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        format!("text:{hex}")
    }

    fn hash_image(image: &ClipboardImage) -> String {
        let mut hasher = Md5::new();
        hasher.update(b"image:");
        hasher.update(image.width.to_le_bytes());
        hasher.update(image.height.to_le_bytes());
        hasher.update(&image.rgba);
        let digest = hasher.finalize();
        let hex = digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        format!("image:{hex}")
    }

    pub fn add_text(&self, text: String) -> bool {
        self.add_content(None, Some(text))
    }

    pub fn add_image(&self, image: ClipboardImage, text: Option<String>) -> bool {
        self.add_content(Some(image), text)
    }

    pub fn add_content(&self, image: Option<ClipboardImage>, text: Option<String>) -> bool {
        let text = text.filter(|text| !text.is_empty());
        let hash = match image.as_ref() {
            Some(image) => Self::hash_image(image),
            None => match text.as_deref() {
                Some(text) => Self::hash_text(text),
                None => return false,
            },
        };
        let mut history = match self.items.lock() {
            Ok(history) => history,
            Err(_) => return false,
        };

        if let Some(item_idx) = history.iter().position(|item| item.hash == hash) {
            let mut item = history.remove(item_idx);
            if image.is_some() {
                item.image = image;
            }
            if let Some(text) = text {
                item.text = text;
            }
            history.insert(0, item);
            return true;
        }

        if history.len() >= MAX_ITEMS {
            history.pop();
        }

        history.insert(
            0,
            ClipboardItem {
                text: text.unwrap_or_default(),
                hash,
                image,
            },
        );
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
        let hash = Self::hash_text(text);
        let Ok(guard) = self.items.lock() else {
            return false;
        };

        guard.iter().any(|item| item.hash == hash)
    }

    pub fn delete(&self, text: &str) -> Result<usize, ClipboardError> {
        let hash = Self::hash_text(text);
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
        let hash = Self::hash_text(text);

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

#[derive(Debug, Clone)]
pub struct ClipboardImage {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl ClipboardImage {
    pub fn from_rgba(rgba: Vec<u8>, width: u32, height: u32) -> Option<Self> {
        let pixel_count = (width as usize).checked_mul(height as usize)?;
        let byte_count = pixel_count.checked_mul(4)?;
        if width == 0 || height == 0 || rgba.len() != byte_count {
            return None;
        }

        Some(Self {
            rgba,
            width,
            height,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ClipboardItem {
    pub text: String,
    pub hash: String,
    #[serde(skip)]
    pub image: Option<ClipboardImage>,
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

        let image = self.app.clipboard().read_image().ok().and_then(|image| {
            ClipboardImage::from_rgba(image.rgba().to_vec(), image.width(), image.height())
        });
        let text = self.app.clipboard().read_text().ok();

        let state = self.app.state::<AppState>();
        let store = &state.clipboard;

        let accepted = match image {
            Some(image) => store.add_image(image, text),
            None => text.map(|text| store.add_text(text)).unwrap_or(false),
        };

        if accepted {
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

#[cfg(test)]
mod tests {
    use super::{ClipboardImage, ClipboardStore, MAX_ITEMS};

    fn image(width: u32, height: u32, byte: u8) -> ClipboardImage {
        ClipboardImage::from_rgba(vec![byte; (width * height * 4) as usize], width, height)
            .expect("test image should be valid")
    }

    #[test]
    fn captures_text_only_content() {
        let store = ClipboardStore::new();

        assert!(store.add_content(None, Some("hello".into())));

        let items = store.list().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].text, "hello");
        assert!(items[0].image.is_none());
        assert!(items[0].hash.starts_with("text:"));
    }

    #[test]
    fn captures_image_only_content_with_pixels_and_dimensions() {
        let store = ClipboardStore::new();
        let expected = image(2, 1, 0x7f);

        assert!(store.add_content(Some(expected.clone()), None));

        let item = store.first().unwrap();
        assert_eq!(item.text, "");
        assert_eq!(item.image.as_ref().unwrap().rgba, expected.rgba);
        assert_eq!(item.image.as_ref().unwrap().width, 2);
        assert_eq!(item.image.as_ref().unwrap().height, 1);
        assert!(item.hash.starts_with("image:"));
    }

    #[test]
    fn captures_mixed_content_as_one_image_primary_entry() {
        let store = ClipboardStore::new();

        assert!(store.add_content(Some(image(1, 1, 1)), Some("fallback".into())));

        let items = store.list().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].text, "fallback");
        assert!(items[0].image.is_some());
    }

    #[test]
    fn deduplicates_images_and_refreshes_the_latest_fallback() {
        let store = ClipboardStore::new();
        let first = image(1, 1, 1);
        let hash = {
            assert!(store.add_image(first.clone(), Some("old".into())));
            store.first().unwrap().hash
        };

        assert!(store.add_image(first, Some("new".into())));

        let items = store.list().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].hash, hash);
        assert_eq!(items[0].text, "new");
    }

    #[test]
    fn keeps_text_and_image_identities_distinct() {
        let store = ClipboardStore::new();

        assert!(store.add_text("same".into()));
        assert!(store.add_image(image(1, 1, 1), Some("same".into())));

        let items = store.list().unwrap();
        assert_eq!(items.len(), 2);
        assert_ne!(items[0].hash, items[1].hash);
    }

    #[test]
    fn evicts_the_oldest_item_across_content_types() {
        let store = ClipboardStore::new();
        for index in 0..MAX_ITEMS {
            assert!(store.add_text(format!("item-{index}")));
        }

        assert!(store.add_image(image(1, 1, 1), None));

        let items = store.list().unwrap();
        assert_eq!(items.len(), MAX_ITEMS);
        assert_eq!(items[0].image.as_ref().unwrap().width, 1);
        assert!(!items.iter().any(|item| item.text == "item-0"));
    }

    #[test]
    fn ignores_empty_or_invalid_content() {
        let store = ClipboardStore::new();

        assert!(!store.add_content(None, Some(String::new())));
        assert!(ClipboardImage::from_rgba(vec![0; 3], 1, 1).is_none());
        assert!(store.list().unwrap().is_empty());
    }
}
