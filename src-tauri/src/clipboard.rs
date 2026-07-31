use md5::{Digest, Md5};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::Serialize;
use std::io;
use std::path::Path;
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
    Database(rusqlite::Error),
    PoisonError,
    ItemNotFound,
}

impl std::fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClipboardError::Database(error) => write!(f, "Clipboard database error: {error}"),
            ClipboardError::PoisonError => write!(f, "Clipboard database lock poisoned"),
            ClipboardError::ItemNotFound => write!(f, "Item not found"),
        }
    }
}

impl std::error::Error for ClipboardError {}

impl From<rusqlite::Error> for ClipboardError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error)
    }
}

#[derive(Debug)]
pub struct ClipboardStore {
    connection: Mutex<Connection>,
}

impl ClipboardStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ClipboardError> {
        Self::from_connection(Connection::open(path)?)
    }

    #[cfg(test)]
    fn new() -> Self {
        Self::from_connection(Connection::open_in_memory().unwrap()).unwrap()
    }

    fn from_connection(connection: Connection) -> Result<Self, ClipboardError> {
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS clipboard_items (
                id INTEGER PRIMARY KEY,
                hash TEXT NOT NULL UNIQUE,
                text TEXT NOT NULL,
                image_rgba BLOB,
                image_width INTEGER,
                image_height INTEGER,
                preview TEXT,
                sort_order INTEGER NOT NULL
            );",
        )?;

        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn get_by_hash(&self, hash: &str) -> Option<ClipboardItem> {
        self.try_get_by_hash(hash).ok().flatten()
    }

    fn try_get_by_hash(&self, hash: &str) -> Result<Option<ClipboardItem>, ClipboardError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| ClipboardError::PoisonError)?;
        let mut statement = connection.prepare(
            "SELECT text, hash, image_rgba, image_width, image_height, preview
             FROM clipboard_items WHERE hash = ?1",
        )?;

        Ok(statement.query_row([hash], row_to_item).optional()?)
    }

    #[allow(dead_code)]
    pub fn exists_by_hash(&self, hash: &str) -> bool {
        self.get_by_hash(hash).is_some()
    }

    pub fn delete_by_hash(&self, hash: &str) -> Result<usize, ClipboardError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| ClipboardError::PoisonError)?;
        let transaction = connection.transaction()?;
        let sort_order: Option<i64> = transaction
            .query_row(
                "SELECT sort_order FROM clipboard_items WHERE hash = ?1",
                [hash],
                |row| row.get(0),
            )
            .optional()?;
        let Some(sort_order) = sort_order else {
            return Err(ClipboardError::ItemNotFound);
        };
        let index = transaction.query_row(
            "SELECT COUNT(*) FROM clipboard_items WHERE sort_order > ?1",
            [sort_order],
            |row| row.get::<_, usize>(0),
        )?;
        transaction.execute("DELETE FROM clipboard_items WHERE hash = ?1", [hash])?;
        transaction.commit()?;

        Ok(index)
    }

    pub fn move_to_top_by_hash(&self, hash: &str) -> Result<(), ClipboardError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| ClipboardError::PoisonError)?;
        let changed = connection.execute(
            "UPDATE clipboard_items
             SET sort_order = (SELECT COALESCE(MAX(sort_order), 0) + 1 FROM clipboard_items)
             WHERE hash = ?1",
            [hash],
        )?;
        if changed == 0 {
            return Err(ClipboardError::ItemNotFound);
        }

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

    #[cfg(test)]
    pub fn add_text(&self, text: String) -> bool {
        self.try_add_text(text).unwrap_or(false)
    }

    pub fn try_add_text(&self, text: String) -> Result<bool, ClipboardError> {
        self.try_add_content(None, Some(text))
    }

    #[cfg(test)]
    pub fn add_image(&self, image: ClipboardImage, text: Option<String>) -> bool {
        self.try_add_image(image, text).unwrap_or(false)
    }

    pub fn try_add_image(
        &self,
        image: ClipboardImage,
        text: Option<String>,
    ) -> Result<bool, ClipboardError> {
        self.try_add_content(Some(image), text)
    }

    #[cfg(test)]
    pub fn add_content(&self, image: Option<ClipboardImage>, text: Option<String>) -> bool {
        self.try_add_content(image, text).unwrap_or(false)
    }

    pub fn try_add_content(
        &self,
        image: Option<ClipboardImage>,
        text: Option<String>,
    ) -> Result<bool, ClipboardError> {
        let text = text.filter(|text| !text.is_empty());
        let hash = match image.as_ref() {
            Some(image) => Self::hash_image(image),
            None => match text.as_deref() {
                Some(text) => Self::hash_text(text),
                None => return Ok(false),
            },
        };
        let preview = image.as_ref().and_then(generate_preview);
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| ClipboardError::PoisonError)?;
        let transaction = connection.transaction()?;
        let exists = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM clipboard_items WHERE hash = ?1)",
            [&hash],
            |row| row.get::<_, bool>(0),
        )?;
        let next_order = next_sort_order(&transaction)?;

        if exists {
            match image.as_ref() {
                Some(image) => {
                    if let Some(text) = text.as_deref() {
                        transaction.execute(
                            "UPDATE clipboard_items SET text = ?1, image_rgba = ?2,
                             image_width = ?3, image_height = ?4, preview = ?5, sort_order = ?6
                             WHERE hash = ?7",
                            params![
                                text,
                                &image.rgba,
                                image.width,
                                image.height,
                                preview,
                                next_order,
                                hash
                            ],
                        )?;
                    } else {
                        transaction.execute(
                            "UPDATE clipboard_items SET image_rgba = ?1, image_width = ?2,
                             image_height = ?3, preview = ?4, sort_order = ?5 WHERE hash = ?6",
                            params![
                                &image.rgba,
                                image.width,
                                image.height,
                                preview,
                                next_order,
                                hash
                            ],
                        )?;
                    }
                }
                None => {
                    transaction.execute(
                        "UPDATE clipboard_items SET text = ?1, sort_order = ?2 WHERE hash = ?3",
                        params![text.as_deref().unwrap_or_default(), next_order, hash],
                    )?;
                }
            }
        } else {
            let (rgba, width, height) = match image.as_ref() {
                Some(image) => (
                    Some(image.rgba.as_slice()),
                    Some(image.width),
                    Some(image.height),
                ),
                None => (None, None, None),
            };
            transaction.execute(
                "INSERT INTO clipboard_items
                 (hash, text, image_rgba, image_width, image_height, preview, sort_order)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    hash,
                    text.as_deref().unwrap_or_default(),
                    rgba,
                    width,
                    height,
                    preview,
                    next_order
                ],
            )?;
        }

        transaction.execute(
            "DELETE FROM clipboard_items WHERE id IN (
                SELECT id FROM clipboard_items ORDER BY sort_order DESC LIMIT -1 OFFSET ?1
            )",
            [MAX_ITEMS],
        )?;
        transaction.commit()?;

        Ok(true)
    }

    pub fn clear(&self) -> Result<(), ClipboardError> {
        self.connection
            .lock()
            .map_err(|_| ClipboardError::PoisonError)?
            .execute("DELETE FROM clipboard_items", [])?;
        Ok(())
    }

    pub fn first(&self) -> Option<ClipboardItem> {
        self.list().ok()?.into_iter().next()
    }

    pub fn list(&self) -> Result<Vec<ClipboardItem>, ClipboardError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| ClipboardError::PoisonError)?;
        let mut statement = connection.prepare(
            "SELECT text, hash, image_rgba, image_width, image_height, preview
             FROM clipboard_items ORDER BY sort_order DESC",
        )?;
        let items = statement
            .query_map([], row_to_item)?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(items)
    }
}

fn next_sort_order(transaction: &Transaction<'_>) -> rusqlite::Result<i64> {
    transaction.query_row(
        "SELECT COALESCE(MAX(sort_order), 0) + 1 FROM clipboard_items",
        [],
        |row| row.get(0),
    )
}

fn row_to_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<ClipboardItem> {
    let rgba: Option<Vec<u8>> = row.get(2)?;
    let width: Option<u32> = row.get(3)?;
    let height: Option<u32> = row.get(4)?;
    let image = match (rgba, width, height) {
        (Some(rgba), Some(width), Some(height)) => ClipboardImage::from_rgba(rgba, width, height),
        _ => None,
    };

    Ok(ClipboardItem {
        text: row.get(0)?,
        hash: row.get(1)?,
        image,
        preview: row.get(5)?,
    })
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
            Some(image) => store.try_add_image(image, text),
            None => match text {
                Some(text) => store.try_add_text(text),
                None => Ok(false),
            },
        };

        match accepted {
            Ok(true) => {
                let _ = self.app.emit_clipboard_changed();
            }
            Ok(false) => {}
            Err(error) => println!("Failed to store clipboard item: {error}"),
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

    #[test]
    fn paste_retrieves_image_item_with_full_resolution_data() {
        let store = ClipboardStore::new();
        let expected = image(4, 3, 0x42);

        assert!(store.add_image(expected.clone(), None));

        let item = store.first().unwrap();
        let retrieved = store.get_by_hash(&item.hash).unwrap();

        assert!(retrieved.image.is_some());
        let img = retrieved.image.unwrap();
        assert_eq!(img.rgba, expected.rgba);
        assert_eq!(img.width, 4);
        assert_eq!(img.height, 3);
    }

    #[test]
    fn paste_retrieves_mixed_item_with_image_and_fallback_text() {
        let store = ClipboardStore::new();

        assert!(store.add_image(image(2, 2, 0xff), Some("alt text".into())));

        let item = store.first().unwrap();
        let retrieved = store.get_by_hash(&item.hash).unwrap();

        assert!(retrieved.image.is_some());
        assert_eq!(retrieved.text, "alt text");
    }

    #[test]
    fn paste_stale_hash_returns_none() {
        let store = ClipboardStore::new();
        assert!(store.add_text("existing".into()));

        assert!(store.get_by_hash("text:nonexistent").is_none());
        assert!(store.get_by_hash("image:nonexistent").is_none());
    }

    #[test]
    fn delete_by_hash_removes_exact_item_for_image() {
        let store = ClipboardStore::new();
        assert!(store.add_image(image(1, 1, 1), None));
        assert!(store.add_text("other".into()));

        let img_hash = store.list().unwrap()[1].hash.clone();

        assert!(store.delete_by_hash(&img_hash).is_ok());
        let items = store.list().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].text, "other");
    }

    #[test]
    fn new_first_item_after_delete_has_image_when_applicable() {
        let store = ClipboardStore::new();
        // Add image first (older), then text (newer, index 0).
        assert!(store.add_image(image(3, 3, 0x77), None));
        assert!(store.add_text("delete-me".into()));

        // Delete the text item at index 0 so the image becomes first.
        let text_hash = store.first().unwrap().hash.clone();
        assert!(store.delete_by_hash(&text_hash).is_ok());

        let new_first = store.first().unwrap();
        assert!(new_first.image.is_some());
        let img = new_first.image.unwrap();
        assert_eq!(img.rgba, vec![0x77; 3 * 3 * 4]);
    }

    #[test]
    fn clears_all_text_and_image_entries() {
        let store = ClipboardStore::new();
        assert!(store.add_text("hello".into()));
        assert!(store.add_image(image(2, 2, 0x11), None));
        assert!(store.add_image(image(1, 1, 0x22), Some("mixed".into())));
        assert_eq!(store.list().unwrap().len(), 3);

        assert!(store.clear().is_ok());
        assert!(store.list().unwrap().is_empty());
        assert!(store.first().is_none());
    }

    #[test]
    fn persists_history_deletions_and_clear_between_connections() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("klipo-clipboard-{unique}.sqlite3"));

        let (image_hash, text_hash) = {
            let store = ClipboardStore::open(&path).unwrap();
            assert!(store.add_text("persistent text".into()));
            let text_hash = store.first().unwrap().hash;
            assert!(store.add_image(image(2, 1, 0x42), Some("fallback".into())));
            (store.first().unwrap().hash, text_hash)
        };

        {
            let store = ClipboardStore::open(&path).unwrap();
            let items = store.list().unwrap();
            assert_eq!(items.len(), 2);
            assert_eq!(items[0].image.as_ref().unwrap().rgba, vec![0x42; 8]);
            store.move_to_top_by_hash(&text_hash).unwrap();
        }

        {
            let store = ClipboardStore::open(&path).unwrap();
            assert_eq!(store.first().unwrap().hash, text_hash);
            store.delete_by_hash(&image_hash).unwrap();
        }

        {
            let store = ClipboardStore::open(&path).unwrap();
            assert_eq!(store.list().unwrap().len(), 1);
            store.clear().unwrap();
        }

        let store = ClipboardStore::open(&path).unwrap();
        assert!(store.list().unwrap().is_empty());
        drop(store);
        let _ = std::fs::remove_file(path);
    }
}
