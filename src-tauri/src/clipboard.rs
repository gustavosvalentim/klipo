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

const THUMBNAIL_MAX_SIZE: u32 = 20;

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

    pub fn get_by_hash(&self, hash: &str) -> Option<ClipboardItem> {
        let guard = self.items.lock().ok()?;
        guard.iter().find(|item| item.hash == hash).cloned()
    }

    #[allow(dead_code)]
    pub fn exists_by_hash(&self, hash: &str) -> bool {
        let Ok(guard) = self.items.lock() else {
            return false;
        };
        guard.iter().any(|item| item.hash == hash)
    }

    pub fn delete_by_hash(&self, hash: &str) -> Result<usize, ClipboardError> {
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

    pub fn move_to_top_by_hash(&self, hash: &str) -> Result<(), ClipboardError> {
        let mut guard = self.items.lock().map_err(|_| ClipboardError::PoisonError)?;

        let item_idx = guard
            .iter()
            .position(|item| item.hash == hash)
            .ok_or(ClipboardError::ItemNotFound)?;

        let item = guard.remove(item_idx);

        guard.insert(0, item);

        Ok(())
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
            if let Some(image) = image {
                item.image = Some(image);
                item.preview = generate_preview(item.image.as_ref().unwrap());
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

        let preview = image.as_ref().and_then(generate_preview);

        history.insert(
            0,
            ClipboardItem {
                text: text.unwrap_or_default(),
                hash,
                image,
                preview,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
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

fn generate_preview(image: &ClipboardImage) -> Option<String> {
    let (new_w, new_h) = if image.width > THUMBNAIL_MAX_SIZE || image.height > THUMBNAIL_MAX_SIZE {
        if image.width > image.height {
            let h = (image.height * THUMBNAIL_MAX_SIZE).max(1) / image.width;
            (THUMBNAIL_MAX_SIZE, h.max(1))
        } else if image.height > image.width {
            let w = (image.width * THUMBNAIL_MAX_SIZE).max(1) / image.height;
            (w.max(1), THUMBNAIL_MAX_SIZE)
        } else {
            (THUMBNAIL_MAX_SIZE, THUMBNAIL_MAX_SIZE)
        }
    } else {
        (image.width, image.height)
    };

    let mut png_bytes = Vec::new();
    {
        use image::imageops::FilterType;
        use image::ImageFormat;
        use image::RgbaImage;
        use std::io::Cursor;

        let img = RgbaImage::from_raw(image.width, image.height, image.rgba.clone())?;
        let thumb = image::imageops::resize(&img, new_w, new_h, FilterType::Nearest);
        thumb
            .write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png)
            .ok()?;
    }

    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&png_bytes);

    Some(format!("data:image/png;base64,{b64}"))
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
        assert!(items[0].preview.is_none());
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

    #[test]
    fn generates_preview_for_image_items() {
        let store = ClipboardStore::new();

        assert!(store.add_content(Some(image(4, 2, 0xab)), None));

        let item = store.first().unwrap();
        let preview = item.preview.expect("image items should have a preview");
        assert!(preview.starts_with("data:image/png;base64,"));
    }

    #[test]
    fn no_preview_for_text_items() {
        let store = ClipboardStore::new();

        assert!(store.add_text("hello".into()));

        let item = store.first().unwrap();
        assert!(item.preview.is_none());
    }

    #[test]
    fn hash_based_get_and_exists() {
        let store = ClipboardStore::new();
        let hash = {
            assert!(store.add_text("item".into()));
            store.first().unwrap().hash.clone()
        };

        assert!(store.exists_by_hash(&hash));
        assert!(!store.exists_by_hash("nonexistent"));

        let item = store.get_by_hash(&hash);
        assert!(item.is_some());
        assert_eq!(item.unwrap().text, "item");
    }

    #[test]
    fn hash_based_delete() {
        let store = ClipboardStore::new();
        assert!(store.add_text("first".into()));
        assert!(store.add_text("second".into()));
        let second_hash = store.first().unwrap().hash.clone();

        assert_eq!(store.delete_by_hash(&second_hash).unwrap(), 0);
        assert_eq!(store.list().unwrap().len(), 1);
        assert_eq!(store.list().unwrap()[0].text, "first");
    }

    #[test]
    fn hash_based_delete_nonexistent() {
        let store = ClipboardStore::new();
        assert!(store.add_text("item".into()));

        assert!(store.delete_by_hash("text:nonexistent").is_err());
        assert_eq!(store.list().unwrap().len(), 1);
    }

    #[test]
    fn hash_based_move_to_top() {
        let store = ClipboardStore::new();
        assert!(store.add_text("first".into()));
        assert!(store.add_text("second".into()));
        let first_hash = {
            let items = store.list().unwrap();
            items[1].hash.clone()
        };

        assert!(store.move_to_top_by_hash(&first_hash).is_ok());
        assert_eq!(store.list().unwrap()[0].text, "first");
    }

    #[test]
    fn hash_based_move_to_top_nonexistent() {
        let store = ClipboardStore::new();
        assert!(store.add_text("item".into()));

        assert!(store.move_to_top_by_hash("text:nonexistent").is_err());
        assert_eq!(store.list().unwrap().len(), 1);
    }
}
