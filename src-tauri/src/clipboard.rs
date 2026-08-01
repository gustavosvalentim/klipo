use serde::Serialize;
use std::sync::Arc;
use std::vec::Vec;

use clipboard_rs::common::RustImage;
use clipboard_rs::{
    Clipboard, ClipboardContent, ClipboardContext, ClipboardHandler, ClipboardWatcher,
    ClipboardWatcherContext, ContentFormat, RustImageData,
};
use log::{debug, error};
use tauri::{Emitter, Manager};

use crate::state::AppState;
use crate::window::get_focused_window;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemClipboardError {
    Initialization,
    Read,
    LockPoisoned,
}

impl std::fmt::Display for SystemClipboardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::Initialization => "Failed to initialize system clipboard",
            Self::Read => "Failed to read system clipboard",
            Self::LockPoisoned => "System clipboard lock is poisoned",
        };
        f.write_str(message)
    }
}

impl std::error::Error for SystemClipboardError {}

#[derive(Debug, Default)]
pub struct ClipboardSnapshot {
    pub image: Option<ClipboardImage>,
    pub text: Option<String>,
}

trait ClipboardBackend: Send {
    fn get(&self, formats: &[ContentFormat])
        -> Result<Vec<ClipboardContent>, SystemClipboardError>;
}

struct NativeClipboardBackend {
    context: ClipboardContext,
}

impl ClipboardBackend for NativeClipboardBackend {
    fn get(
        &self,
        formats: &[ContentFormat],
    ) -> Result<Vec<ClipboardContent>, SystemClipboardError> {
        self.context
            .get(formats)
            .map_err(|_| SystemClipboardError::Read)
    }
}

pub struct SystemClipboard {
    context: std::sync::Mutex<Box<dyn ClipboardBackend>>,
}

impl SystemClipboard {
    pub fn new() -> Result<Self, SystemClipboardError> {
        let context = ClipboardContext::new().map_err(|_| SystemClipboardError::Initialization)?;
        Ok(Self {
            context: std::sync::Mutex::new(Box::new(NativeClipboardBackend { context })),
        })
    }

    pub fn read(&self) -> Result<ClipboardSnapshot, SystemClipboardError> {
        let contents = {
            let context = self
                .context
                .lock()
                .map_err(|_| SystemClipboardError::LockPoisoned)?;
            context.get(&[ContentFormat::Image, ContentFormat::Text])?
        };

        Ok(snapshot_from_contents(contents))
    }

    #[cfg(test)]
    fn from_backend(backend: impl ClipboardBackend + 'static) -> Self {
        Self {
            context: std::sync::Mutex::new(Box::new(backend)),
        }
    }
}

fn snapshot_from_contents(contents: Vec<ClipboardContent>) -> ClipboardSnapshot {
    let mut snapshot = ClipboardSnapshot::default();

    for content in contents {
        match content {
            ClipboardContent::Image(image) if snapshot.image.is_none() => {
                snapshot.image = clipboard_image_from_rust(image);
            }
            ClipboardContent::Text(text) if snapshot.text.is_none() && !text.is_empty() => {
                snapshot.text = Some(text);
            }
            _ => {}
        }
    }

    snapshot
}

fn clipboard_image_from_rust(image: RustImageData) -> Option<ClipboardImage> {
    let (width, height) = image.get_size();
    let rgba = image.to_rgba8().ok()?.into_raw();
    ClipboardImage::from_rgba(rgba, width, height)
}

fn store_snapshot(
    store: &crate::storage::ClipboardStore,
    snapshot: ClipboardSnapshot,
) -> Result<bool, crate::storage::ClipboardError> {
    match snapshot.image {
        Some(image) => store.try_add_image(image, snapshot.text),
        None => match snapshot.text {
            Some(text) => store.try_add_text(text),
            None => Ok(false),
        },
    }
}

#[derive(Debug)]
enum ClipboardCaptureError {
    Read(SystemClipboardError),
    Store(crate::storage::ClipboardError),
}

fn capture_clipboard_change(
    system_clipboard: &SystemClipboard,
    store: &crate::storage::ClipboardStore,
) -> Result<bool, ClipboardCaptureError> {
    let snapshot = system_clipboard
        .read()
        .map_err(ClipboardCaptureError::Read)?;
    store_snapshot(store, snapshot).map_err(ClipboardCaptureError::Store)
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

#[derive(Debug)]
pub struct ClipboardWatcherInitializationError;

impl std::fmt::Display for ClipboardWatcherInitializationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Failed to initialize clipboard watcher")
    }
}

impl std::error::Error for ClipboardWatcherInitializationError {}

pub struct ClipboardEventsListener {
    watcher: ClipboardWatcherContext<ClipboardEventsHandler>,
}

impl ClipboardEventsListener {
    pub fn new(
        app_handler: tauri::AppHandle,
    ) -> Result<ClipboardEventsListener, ClipboardWatcherInitializationError> {
        let mut watcher =
            ClipboardWatcherContext::new().map_err(|_| ClipboardWatcherInitializationError)?;
        watcher.add_handler(ClipboardEventsHandler::new(Arc::new(app_handler)));
        Ok(Self { watcher })
    }

    pub fn start(mut self) {
        self.watcher.start_watch();
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
    fn on_clipboard_change(&mut self) {
        debug!("Clipboard changed");

        let klipo_pid = std::process::id();
        let focused_window_pid = get_focused_window();

        if let Some(focused_window_pid) = focused_window_pid {
            if focused_window_pid as u32 == klipo_pid {
                return;
            }
        }

        let state = self.app.state::<AppState>();
        let accepted = capture_clipboard_change(&state.system_clipboard, &state.clipboard);

        match accepted {
            Ok(true) => {
                if let Err(error) = self.app.emit_clipboard_changed() {
                    error!(error:debug = error; "Failed to emit clipboard changed event");
                }
            }
            Ok(false) => {}
            Err(ClipboardCaptureError::Read(error)) => {
                error!(error:% = error; "Failed to read system clipboard");
            }
            Err(ClipboardCaptureError::Store(error)) => {
                error!(error:debug = error; "Failed to store clipboard item");
            }
        }
    }
}

const THUMBNAIL_MAX_SIZE: u32 = 20;

pub(crate) fn generate_preview(image: &ClipboardImage) -> Option<String> {
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
    use super::{
        capture_clipboard_change, store_snapshot, ClipboardBackend, ClipboardCaptureError,
        ClipboardContent, ClipboardSnapshot, ContentFormat, RustImage, RustImageData,
        SystemClipboard, SystemClipboardError,
    };
    use crate::storage::ClipboardStore;
    use image::{DynamicImage, RgbaImage};
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    struct FakeClipboard {
        responses: Mutex<VecDeque<Result<Vec<ClipboardContent>, SystemClipboardError>>>,
        requested_formats: Arc<Mutex<Vec<(bool, bool)>>>,
    }

    impl FakeClipboard {
        fn new(
            responses: Vec<Result<Vec<ClipboardContent>, SystemClipboardError>>,
        ) -> (Self, Arc<Mutex<Vec<(bool, bool)>>>) {
            let requested_formats = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    responses: Mutex::new(responses.into()),
                    requested_formats: Arc::clone(&requested_formats),
                },
                requested_formats,
            )
        }
    }

    impl ClipboardBackend for FakeClipboard {
        fn get(
            &self,
            formats: &[ContentFormat],
        ) -> Result<Vec<ClipboardContent>, SystemClipboardError> {
            let has_image = formats
                .iter()
                .any(|format| matches!(format, ContentFormat::Image));
            let has_text = formats
                .iter()
                .any(|format| matches!(format, ContentFormat::Text));
            self.requested_formats
                .lock()
                .unwrap()
                .push((has_image, has_text));
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(Vec::new()))
        }
    }

    fn rust_image(rgba: Vec<u8>, width: u32, height: u32) -> RustImageData {
        let image = RgbaImage::from_raw(width, height, rgba).unwrap();
        RustImageData::from_dynamic_image(DynamicImage::ImageRgba8(image))
    }

    #[test]
    fn reads_text_only_content() {
        let (backend, requested_formats) =
            FakeClipboard::new(vec![Ok(vec![ClipboardContent::Text("hello".into())])]);
        let clipboard = SystemClipboard::from_backend(backend);

        let snapshot = clipboard.read().unwrap();

        assert_eq!(snapshot.text.as_deref(), Some("hello"));
        assert!(snapshot.image.is_none());
        assert_eq!(*requested_formats.lock().unwrap(), vec![(true, true)]);
    }

    #[test]
    fn reads_image_only_content_without_changing_pixels_or_dimensions() {
        let rgba = vec![1, 2, 3, 4, 5, 6, 7, 8];
        let (backend, _) = FakeClipboard::new(vec![Ok(vec![ClipboardContent::Image(rust_image(
            rgba.clone(),
            2,
            1,
        ))])]);
        let clipboard = SystemClipboard::from_backend(backend);

        let snapshot = clipboard.read().unwrap();
        let image = snapshot.image.unwrap();

        assert_eq!(image.rgba, rgba);
        assert_eq!((image.width, image.height), (2, 1));
        assert!(snapshot.text.is_none());
    }

    #[test]
    fn reads_image_and_text_as_one_snapshot() {
        let (backend, _) = FakeClipboard::new(vec![Ok(vec![
            ClipboardContent::Image(rust_image(vec![1, 2, 3, 4], 1, 1)),
            ClipboardContent::Text("fallback".into()),
        ])]);
        let clipboard = SystemClipboard::from_backend(backend);

        let snapshot = clipboard.read().unwrap();

        assert_eq!(snapshot.text.as_deref(), Some("fallback"));
        assert_eq!(snapshot.image.unwrap().rgba, vec![1, 2, 3, 4]);
    }

    #[test]
    fn ignores_empty_text_unsupported_content_and_invalid_images() {
        let (backend, _) = FakeClipboard::new(vec![Ok(vec![
            ClipboardContent::Text(String::new()),
            ClipboardContent::Other("application/octet-stream".into(), vec![1, 2, 3]),
            ClipboardContent::Image(RustImageData::empty()),
        ])]);
        let clipboard = SystemClipboard::from_backend(backend);

        let snapshot = clipboard.read().unwrap();

        assert!(snapshot.text.is_none());
        assert!(snapshot.image.is_none());
    }

    #[test]
    fn keeps_available_content_when_another_representation_is_unavailable() {
        let (backend, _) =
            FakeClipboard::new(vec![Ok(vec![ClipboardContent::Text("available".into())])]);
        let clipboard = SystemClipboard::from_backend(backend);

        let snapshot = clipboard.read().unwrap();

        assert_eq!(snapshot.text.as_deref(), Some("available"));
        assert!(snapshot.image.is_none());
    }

    #[test]
    fn read_failure_does_not_poison_later_reads() {
        let (backend, _) = FakeClipboard::new(vec![
            Err(SystemClipboardError::Read),
            Ok(vec![ClipboardContent::Text("later".into())]),
        ]);
        let clipboard = SystemClipboard::from_backend(backend);

        assert!(matches!(clipboard.read(), Err(SystemClipboardError::Read)));
        assert_eq!(clipboard.read().unwrap().text.as_deref(), Some("later"));
    }

    #[test]
    fn empty_snapshot_is_not_stored() {
        let store = ClipboardStore::new();

        assert!(!store_snapshot(&store, ClipboardSnapshot::default()).unwrap());
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn capture_flow_ignores_empty_content_and_accepts_the_next_change() {
        let (backend, _) = FakeClipboard::new(vec![
            Ok(Vec::new()),
            Ok(vec![ClipboardContent::Text("later".into())]),
        ]);
        let system_clipboard = SystemClipboard::from_backend(backend);
        let store = ClipboardStore::new();

        assert!(!capture_clipboard_change(&system_clipboard, &store).unwrap());
        assert!(capture_clipboard_change(&system_clipboard, &store).unwrap());
        assert_eq!(store.list().unwrap().len(), 1);
        assert_eq!(store.first().unwrap().text, "later");
    }

    #[test]
    fn capture_flow_recovers_after_a_read_failure() {
        let (backend, _) = FakeClipboard::new(vec![
            Err(SystemClipboardError::Read),
            Ok(vec![ClipboardContent::Text("recovered".into())]),
        ]);
        let system_clipboard = SystemClipboard::from_backend(backend);
        let store = ClipboardStore::new();

        assert!(matches!(
            capture_clipboard_change(&system_clipboard, &store),
            Err(ClipboardCaptureError::Read(SystemClipboardError::Read))
        ));
        assert!(capture_clipboard_change(&system_clipboard, &store).unwrap());
        assert_eq!(store.first().unwrap().text, "recovered");
    }
}
