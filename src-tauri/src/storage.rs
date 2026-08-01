use md5::{Digest, Md5};
use rusqlite::{params, Connection, OptionalExtension};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use std::vec::Vec;

use crate::clipboard::{generate_preview, ClipboardImage, ClipboardItem};

const MAX_ITEMS: usize = 120;

#[derive(Debug)]
pub enum ClipboardError {
    Database(rusqlite::Error),
    Io(io::Error),
    PoisonError,
    ItemNotFound,
}

impl std::fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClipboardError::Database(error) => write!(f, "Clipboard database error: {error}"),
            ClipboardError::Io(error) => write!(f, "Clipboard storage error: {error}"),
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

impl From<io::Error> for ClipboardError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug)]
pub struct ClipboardStore {
    connection: Mutex<Connection>,
    images_directory: PathBuf,
    #[cfg(test)]
    remove_images_on_drop: bool,
}

impl ClipboardStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ClipboardError> {
        let path = path.as_ref();
        let images_directory = path.with_file_name("clipboard-images");
        Self::from_connection(Connection::open(path)?, images_directory)
    }

    #[cfg(test)]
    pub(crate) fn new() -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

        let id = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let images_directory =
            std::env::temp_dir().join(format!("klipo-images-{}-{id}", std::process::id()));
        let mut store =
            Self::from_connection(Connection::open_in_memory().unwrap(), images_directory).unwrap();
        store.remove_images_on_drop = true;
        store
    }

    fn from_connection(
        mut connection: Connection,
        images_directory: PathBuf,
    ) -> Result<Self, ClipboardError> {
        fs::create_dir_all(&images_directory)?;
        connection.execute_batch(
            "PRAGMA secure_delete = ON;
             CREATE TABLE IF NOT EXISTS clipboard_items (
                id INTEGER PRIMARY KEY,
                hash TEXT NOT NULL UNIQUE,
                text TEXT NOT NULL,
                image_file TEXT,
                image_width INTEGER,
                image_height INTEGER,
                preview TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );",
        )?;
        migrate_sort_order(&mut connection)?;

        Ok(Self {
            connection: Mutex::new(connection),
            images_directory,
            #[cfg(test)]
            remove_images_on_drop: false,
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
            "SELECT text, hash, image_file, image_width, image_height, preview
             FROM clipboard_items WHERE hash = ?1",
        )?;

        Ok(statement
            .query_row([hash], |row| self.row_to_item(row, true))
            .optional()?)
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
        let stored_item: Option<(i64, Option<String>)> = transaction
            .query_row(
                "SELECT updated_at, image_file FROM clipboard_items WHERE hash = ?1",
                [hash],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((updated_at, image_file)) = stored_item else {
            return Err(ClipboardError::ItemNotFound);
        };
        let index = transaction.query_row(
            "SELECT COUNT(*) FROM clipboard_items WHERE updated_at > ?1",
            [updated_at],
            |row| row.get::<_, usize>(0),
        )?;
        transaction.execute("DELETE FROM clipboard_items WHERE hash = ?1", [hash])?;
        transaction.commit()?;
        if let Some(image_file) = image_file {
            remove_file_if_exists(&self.images_directory.join(image_file))?;
        }

        Ok(index)
    }

    pub fn move_to_top_by_hash(&self, hash: &str) -> Result<(), ClipboardError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| ClipboardError::PoisonError)?;
        let updated_at = now_ns();
        let changed = connection.execute(
            "UPDATE clipboard_items SET updated_at = ?1 WHERE hash = ?2",
            params![updated_at, hash],
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
        let image_file = match image.as_ref() {
            Some(image) => Some(self.write_image(&hash, image)?),
            None => None,
        };
        let transaction = connection.transaction()?;
        let updated_at = now_ns();
        let (width, height) = match image.as_ref() {
            Some(image) => (Some(image.width), Some(image.height)),
            None => (None, None),
        };

        let _: i64 = transaction.query_row(
            "INSERT INTO clipboard_items
             (hash, text, image_file, image_width, image_height, preview, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(hash) DO UPDATE SET
                 text = CASE WHEN excluded.text = '' THEN clipboard_items.text ELSE excluded.text END,
                 image_file = excluded.image_file,
                 image_width = excluded.image_width,
                 image_height = excluded.image_height,
                 preview = excluded.preview,
                 updated_at = excluded.updated_at
             RETURNING id",
            params![
                hash,
                text.as_deref().unwrap_or_default(),
                image_file,
                width,
                height,
                preview,
                updated_at,
                updated_at
            ],
            |row| row.get(0),
        )?;

        let evicted_files = {
            let mut statement = transaction.prepare(
                "SELECT image_file FROM clipboard_items
                 ORDER BY updated_at DESC LIMIT -1 OFFSET ?1",
            )?;
            let files = statement
                .query_map([MAX_ITEMS], |row| row.get::<_, Option<String>>(0))?
                .filter_map(Result::transpose)
                .collect::<Result<Vec<_>, _>>()?;
            files
        };
        transaction.execute(
            "DELETE FROM clipboard_items WHERE id IN (
                SELECT id FROM clipboard_items ORDER BY updated_at DESC LIMIT -1 OFFSET ?1
            )",
            [MAX_ITEMS],
        )?;
        transaction.commit()?;
        for image_file in evicted_files {
            remove_file_if_exists(&self.images_directory.join(image_file))?;
        }

        Ok(true)
    }

    pub fn clear(&self) -> Result<(), ClipboardError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| ClipboardError::PoisonError)?;
        connection.execute("DELETE FROM clipboard_items", [])?;
        match fs::remove_dir_all(&self.images_directory) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        fs::create_dir_all(&self.images_directory)?;
        Ok(())
    }

    pub fn first(&self) -> Option<ClipboardItem> {
        let connection = self.connection.lock().ok()?;
        let mut statement = connection
            .prepare(
                "SELECT text, hash, image_file, image_width, image_height, preview
                 FROM clipboard_items ORDER BY updated_at DESC LIMIT 1",
            )
            .ok()?;
        statement
            .query_row([], |row| self.row_to_item(row, true))
            .optional()
            .ok()
            .flatten()
    }

    #[cfg(test)]
    pub fn list(&self) -> Result<Vec<ClipboardItem>, ClipboardError> {
        self.list_items(true)
    }

    pub fn list_for_display(&self) -> Result<Vec<ClipboardItem>, ClipboardError> {
        self.list_items(false)
    }

    fn list_items(&self, load_images: bool) -> Result<Vec<ClipboardItem>, ClipboardError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| ClipboardError::PoisonError)?;
        let mut statement = connection.prepare(
            "SELECT text, hash, image_file, image_width, image_height, preview
             FROM clipboard_items ORDER BY updated_at DESC",
        )?;
        let items = statement
            .query_map([], |row| self.row_to_item(row, load_images))?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(items)
    }

    fn write_image(&self, hash: &str, image: &ClipboardImage) -> Result<String, ClipboardError> {
        let filename = format!("{}.rgba", hash.trim_start_matches("image:"));
        let path = self.images_directory.join(&filename);
        let temporary = path.with_extension("rgba.tmp");
        fs::write(&temporary, &image.rgba)?;
        fs::rename(temporary, path)?;
        Ok(filename)
    }

    fn row_to_item(
        &self,
        row: &rusqlite::Row<'_>,
        load_image: bool,
    ) -> rusqlite::Result<ClipboardItem> {
        let image_file: Option<String> = row.get(2)?;
        let width: Option<u32> = row.get(3)?;
        let height: Option<u32> = row.get(4)?;
        let image = if load_image {
            match (image_file, width, height) {
                (Some(image_file), Some(width), Some(height)) => {
                    fs::read(self.images_directory.join(image_file))
                        .ok()
                        .and_then(|rgba| ClipboardImage::from_rgba(rgba, width, height))
                }
                _ => None,
            }
        } else {
            None
        };

        Ok(ClipboardItem {
            text: row.get(0)?,
            hash: row.get(1)?,
            image,
            preview: row.get(5)?,
        })
    }
}

#[cfg(test)]
impl Drop for ClipboardStore {
    fn drop(&mut self) {
        if self.remove_images_on_drop {
            let _ = fs::remove_dir_all(&self.images_directory);
        }
    }
}

fn remove_file_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn now_ns() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .try_into()
        .unwrap_or(i64::MAX)
}

fn migrate_sort_order(connection: &mut Connection) -> rusqlite::Result<()> {
    let has_sort_order = {
        let mut statement = connection.prepare("PRAGMA table_info(clipboard_items)")?;
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<Vec<_>, _>>()?;
        columns.into_iter().any(|name| name == "sort_order")
    };
    if !has_sort_order {
        return Ok(());
    }

    let transaction = connection.transaction()?;
    transaction.execute(
        "ALTER TABLE clipboard_items ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0",
        [],
    )?;
    transaction.execute(
        "ALTER TABLE clipboard_items ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0",
        [],
    )?;

    let ids = {
        let mut statement = transaction
            .prepare("SELECT id FROM clipboard_items ORDER BY sort_order DESC, id DESC")?;
        let ids = statement
            .query_map([], |row| row.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        ids
    };
    let now = now_ns();
    for (offset, id) in ids.into_iter().enumerate() {
        let timestamp = now.saturating_sub(offset as i64);
        transaction.execute(
            "UPDATE clipboard_items SET created_at = ?1, updated_at = ?1 WHERE id = ?2",
            params![timestamp, id],
        )?;
    }
    transaction.execute("ALTER TABLE clipboard_items DROP COLUMN sort_order", [])?;
    transaction.commit()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{ClipboardStore, Connection, MAX_ITEMS};
    use crate::clipboard::ClipboardImage;

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
    fn evicting_an_image_removes_its_file() {
        let store = ClipboardStore::new();
        assert!(store.add_image(image(1, 1, 1), None));
        assert_eq!(fs::read_dir(&store.images_directory).unwrap().count(), 1);

        for index in 0..MAX_ITEMS {
            assert!(store.add_text(format!("item-{index}")));
        }

        assert_eq!(store.list().unwrap().len(), MAX_ITEMS);
        assert_eq!(fs::read_dir(&store.images_directory).unwrap().count(), 0);
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
    fn display_list_does_not_load_full_resolution_image_files() {
        let store = ClipboardStore::new();
        assert!(store.add_image(image(2, 2, 0x42), None));

        let item = store.list_for_display().unwrap().remove(0);

        assert!(item.image.is_none());
        assert!(item.preview.is_some());
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
        assert_eq!(fs::read_dir(&store.images_directory).unwrap().count(), 2);

        assert!(store.clear().is_ok());
        assert!(store.list().unwrap().is_empty());
        assert!(store.first().is_none());
        assert_eq!(fs::read_dir(&store.images_directory).unwrap().count(), 0);
    }

    #[test]
    fn stores_creation_and_update_timestamps() {
        let store = ClipboardStore::new();
        assert!(store.add_text("timestamped".into()));
        let hash = store.first().unwrap().hash;

        let (created_at, first_updated_at) = store
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT created_at, updated_at FROM clipboard_items WHERE hash = ?1",
                [&hash],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .unwrap();
        assert!(created_at > 0);
        assert_eq!(created_at, first_updated_at);

        assert!(store.add_text("timestamped".into()));
        let (created_at_after, updated_at_after) = store
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT created_at, updated_at FROM clipboard_items WHERE hash = ?1",
                [&hash],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .unwrap();
        assert_eq!(created_at_after, created_at);
        assert!(updated_at_after > first_updated_at);
    }

    #[test]
    fn migrates_legacy_sort_order_to_timestamps() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("klipo-legacy-{unique}"));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("clipboard.sqlite3");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE clipboard_items (
                    id INTEGER PRIMARY KEY,
                    hash TEXT NOT NULL UNIQUE,
                    text TEXT NOT NULL,
                    image_file TEXT,
                    image_width INTEGER,
                    image_height INTEGER,
                    preview TEXT,
                    sort_order INTEGER NOT NULL
                );
                INSERT INTO clipboard_items (hash, text, sort_order)
                VALUES ('text:old', 'old', 1), ('text:new', 'new', 2);",
            )
            .unwrap();
        drop(connection);

        let store = ClipboardStore::open(&path).unwrap();
        let items = store.list().unwrap();
        assert_eq!(
            items
                .iter()
                .map(|item| item.text.as_str())
                .collect::<Vec<_>>(),
            ["new", "old"]
        );

        let columns = store
            .connection
            .lock()
            .unwrap()
            .prepare("PRAGMA table_info(clipboard_items)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(columns.iter().any(|column| column == "created_at"));
        assert!(columns.iter().any(|column| column == "updated_at"));
        assert!(!columns.iter().any(|column| column == "sort_order"));

        drop(store);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn persists_history_deletions_and_clear_between_connections() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("klipo-clipboard-{unique}"));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("clipboard.sqlite3");
        let images_directory = directory.join("clipboard-images");

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
            assert_eq!(fs::read_dir(&images_directory).unwrap().count(), 1);
            store.move_to_top_by_hash(&text_hash).unwrap();
        }

        {
            let store = ClipboardStore::open(&path).unwrap();
            assert_eq!(store.first().unwrap().hash, text_hash);
            store.delete_by_hash(&image_hash).unwrap();
            assert_eq!(fs::read_dir(&images_directory).unwrap().count(), 0);
        }

        {
            let store = ClipboardStore::open(&path).unwrap();
            assert_eq!(store.list().unwrap().len(), 1);
            store.clear().unwrap();
        }

        let store = ClipboardStore::open(&path).unwrap();
        assert!(store.list().unwrap().is_empty());
        drop(store);
        let _ = fs::remove_dir_all(directory);
    }
}
