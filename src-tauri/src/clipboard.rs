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
    Write,
    Verification,
    InvalidImage,
    EmptyContent,
    LockPoisoned,
}

impl std::fmt::Display for SystemClipboardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::Initialization => "Failed to initialize system clipboard",
            Self::Read => "Failed to read system clipboard",
            Self::Write => "Failed to write system clipboard",
            Self::Verification => "System clipboard write verification failed",
            Self::InvalidImage => "Invalid clipboard image",
            Self::EmptyContent => "No clipboard content to write",
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
    fn has(&self, format: ContentFormat) -> bool;
    fn set(&self, contents: Vec<ClipboardContent>) -> Result<(), SystemClipboardError>;
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

    fn has(&self, format: ContentFormat) -> bool {
        self.context.has(format)
    }

    fn set(&self, contents: Vec<ClipboardContent>) -> Result<(), SystemClipboardError> {
        self.context
            .set(contents)
            .map_err(|_| SystemClipboardError::Write)
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

    pub fn write_item(&self, item: &ClipboardItem) -> Result<(), SystemClipboardError> {
        let text = (!item.text.is_empty()).then_some(item.text.as_str());

        match (item.image.as_ref(), text) {
            (Some(image), Some(text)) => self.write_mixed(rust_image_from_clipboard(image), text),
            (Some(image), None) => {
                let image = rust_image_from_clipboard(image)?;
                self.write_and_verify(vec![ClipboardContent::Image(image)], true, false)
            }
            (None, Some(text)) => {
                self.write_and_verify(vec![ClipboardContent::Text(text.to_owned())], false, true)
            }
            (None, None) => Err(SystemClipboardError::EmptyContent),
        }
    }

    fn write_mixed(
        &self,
        image: Result<RustImageData, SystemClipboardError>,
        text: &str,
    ) -> Result<(), SystemClipboardError> {
        let context = self
            .context
            .lock()
            .map_err(|_| SystemClipboardError::LockPoisoned)?;

        let mixed_result = match image {
            Ok(image) => write_and_verify(
                context.as_ref(),
                vec![
                    ClipboardContent::Image(image),
                    ClipboardContent::Text(text.to_owned()),
                ],
                true,
                true,
            ),
            Err(error) => Err(error),
        };

        if mixed_result.is_ok() {
            Ok(())
        } else {
            write_and_verify(
                context.as_ref(),
                vec![ClipboardContent::Text(text.to_owned())],
                false,
                true,
            )
        }
    }

    fn write_and_verify(
        &self,
        contents: Vec<ClipboardContent>,
        requires_image: bool,
        requires_text: bool,
    ) -> Result<(), SystemClipboardError> {
        let context = self
            .context
            .lock()
            .map_err(|_| SystemClipboardError::LockPoisoned)?;
        write_and_verify(context.as_ref(), contents, requires_image, requires_text)
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

fn rust_image_from_clipboard(
    image: &ClipboardImage,
) -> Result<RustImageData, SystemClipboardError> {
    let rgba = ::image::RgbaImage::from_raw(image.width, image.height, image.rgba.clone())
        .ok_or(SystemClipboardError::InvalidImage)?;
    Ok(RustImageData::from_dynamic_image(
        ::image::DynamicImage::ImageRgba8(rgba),
    ))
}

fn write_and_verify(
    context: &dyn ClipboardBackend,
    contents: Vec<ClipboardContent>,
    requires_image: bool,
    requires_text: bool,
) -> Result<(), SystemClipboardError> {
    context.set(contents)?;

    let image_available = !requires_image || context.has(ContentFormat::Image);
    let text_available = !requires_text || context.has(ContentFormat::Text);
    if image_available && text_available {
        Ok(())
    } else {
        Err(SystemClipboardError::Verification)
    }
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
        ClipboardContent, ClipboardImage, ClipboardItem, ClipboardSnapshot, ContentFormat,
        RustImage, RustImageData, SystemClipboard, SystemClipboardError,
    };
    use crate::storage::ClipboardStore;
    use image::{DynamicImage, RgbaImage};
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    #[derive(Debug, PartialEq)]
    enum FakeWriteContent {
        Image {
            rgba: Vec<u8>,
            width: u32,
            height: u32,
        },
        Text(String),
    }

    #[derive(Default)]
    struct FakeClipboardObservations {
        requested_formats: Vec<(bool, bool)>,
        writes: Vec<Vec<FakeWriteContent>>,
        verified_formats: Vec<ContentFormatKind>,
    }

    #[derive(Debug, PartialEq)]
    enum ContentFormatKind {
        Image,
        Text,
    }

    struct FakeClipboard {
        responses: Mutex<VecDeque<Result<Vec<ClipboardContent>, SystemClipboardError>>>,
        observations: Arc<Mutex<FakeClipboardObservations>>,
        set_results: Mutex<VecDeque<Result<(), SystemClipboardError>>>,
        has_results: Mutex<VecDeque<bool>>,
    }

    impl FakeClipboard {
        fn new(
            responses: Vec<Result<Vec<ClipboardContent>, SystemClipboardError>>,
        ) -> (Self, Arc<Mutex<FakeClipboardObservations>>) {
            Self::with_write_behavior(responses, Vec::new(), Vec::new())
        }

        fn with_write_behavior(
            responses: Vec<Result<Vec<ClipboardContent>, SystemClipboardError>>,
            set_results: Vec<Result<(), SystemClipboardError>>,
            has_results: Vec<bool>,
        ) -> (Self, Arc<Mutex<FakeClipboardObservations>>) {
            let observations = Arc::new(Mutex::new(FakeClipboardObservations::default()));
            (
                Self {
                    responses: Mutex::new(responses.into()),
                    observations: Arc::clone(&observations),
                    set_results: Mutex::new(set_results.into()),
                    has_results: Mutex::new(has_results.into()),
                },
                observations,
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
            self.observations
                .lock()
                .unwrap()
                .requested_formats
                .push((has_image, has_text));
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(Vec::new()))
        }

        fn has(&self, format: ContentFormat) -> bool {
            let format_kind = match format {
                ContentFormat::Image => ContentFormatKind::Image,
                ContentFormat::Text => ContentFormatKind::Text,
                _ => unreachable!("write verification only asks for image and text"),
            };
            self.observations
                .lock()
                .unwrap()
                .verified_formats
                .push(format_kind);
            self.has_results.lock().unwrap().pop_front().unwrap_or(true)
        }

        fn set(&self, contents: Vec<ClipboardContent>) -> Result<(), SystemClipboardError> {
            let write = contents
                .into_iter()
                .map(|content| match content {
                    ClipboardContent::Image(image) => {
                        let (width, height) = image.get_size();
                        FakeWriteContent::Image {
                            rgba: image.to_rgba8().unwrap().into_raw(),
                            width,
                            height,
                        }
                    }
                    ClipboardContent::Text(text) => FakeWriteContent::Text(text),
                    _ => unreachable!("write tests only use image and text"),
                })
                .collect();
            self.observations.lock().unwrap().writes.push(write);
            self.set_results
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Ok(()))
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
        assert_eq!(
            requested_formats.lock().unwrap().requested_formats,
            vec![(true, true)]
        );
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

    fn item(text: &str, image: Option<ClipboardImage>) -> ClipboardItem {
        ClipboardItem {
            text: text.into(),
            hash: "test-hash".into(),
            image,
            preview: None,
        }
    }

    fn clipboard_image(rgba: Vec<u8>, width: u32, height: u32) -> ClipboardImage {
        ClipboardImage::from_rgba(rgba, width, height).unwrap()
    }

    #[test]
    fn writes_text_once_and_verifies_text() {
        let (backend, observations) =
            FakeClipboard::with_write_behavior(Vec::new(), Vec::new(), vec![true]);
        let clipboard = SystemClipboard::from_backend(backend);

        clipboard.write_item(&item("hello", None)).unwrap();

        let observations = observations.lock().unwrap();
        assert_eq!(
            observations.writes,
            vec![vec![FakeWriteContent::Text("hello".into())]]
        );
        assert_eq!(observations.verified_formats, vec![ContentFormatKind::Text]);
    }

    #[test]
    fn writes_image_once_with_original_pixels_dimensions_and_alpha() {
        let rgba = vec![1, 2, 3, 4, 5, 6, 7, 128];
        let (backend, observations) =
            FakeClipboard::with_write_behavior(Vec::new(), Vec::new(), vec![true]);
        let clipboard = SystemClipboard::from_backend(backend);

        clipboard
            .write_item(&item("", Some(clipboard_image(rgba.clone(), 2, 1))))
            .unwrap();

        let observations = observations.lock().unwrap();
        assert_eq!(
            observations.writes,
            vec![vec![FakeWriteContent::Image {
                rgba,
                width: 2,
                height: 1,
            }]]
        );
        assert_eq!(
            observations.verified_formats,
            vec![ContentFormatKind::Image]
        );
    }

    #[test]
    fn writes_mixed_content_in_one_image_first_operation() {
        let (backend, observations) =
            FakeClipboard::with_write_behavior(Vec::new(), Vec::new(), vec![true, true]);
        let clipboard = SystemClipboard::from_backend(backend);

        clipboard
            .write_item(&item(
                "fallback",
                Some(clipboard_image(vec![10, 20, 30, 40], 1, 1)),
            ))
            .unwrap();

        let observations = observations.lock().unwrap();
        assert_eq!(
            observations.writes,
            vec![vec![
                FakeWriteContent::Image {
                    rgba: vec![10, 20, 30, 40],
                    width: 1,
                    height: 1,
                },
                FakeWriteContent::Text("fallback".into()),
            ]]
        );
        assert_eq!(
            observations.verified_formats,
            vec![ContentFormatKind::Image, ContentFormatKind::Text]
        );
    }

    #[test]
    fn incomplete_mixed_write_uses_one_verified_text_fallback() {
        let (backend, observations) = FakeClipboard::with_write_behavior(
            Vec::new(),
            vec![Ok(()), Ok(())],
            vec![true, false, true],
        );
        let clipboard = SystemClipboard::from_backend(backend);

        clipboard
            .write_item(&item(
                "fallback",
                Some(clipboard_image(vec![10, 20, 30, 40], 1, 1)),
            ))
            .unwrap();

        let observations = observations.lock().unwrap();
        assert_eq!(observations.writes.len(), 2);
        assert_eq!(
            observations.writes[1],
            vec![FakeWriteContent::Text("fallback".into())]
        );
        assert_eq!(
            observations.verified_formats,
            vec![
                ContentFormatKind::Image,
                ContentFormatKind::Text,
                ContentFormatKind::Text,
            ]
        );
    }

    #[test]
    fn failed_mixed_write_uses_fallback_once() {
        let (backend, observations) = FakeClipboard::with_write_behavior(
            Vec::new(),
            vec![Err(SystemClipboardError::Write), Ok(())],
            vec![true],
        );
        let clipboard = SystemClipboard::from_backend(backend);

        clipboard
            .write_item(&item(
                "fallback",
                Some(clipboard_image(vec![10, 20, 30, 40], 1, 1)),
            ))
            .unwrap();

        let observations = observations.lock().unwrap();
        assert_eq!(observations.writes.len(), 2);
        assert_eq!(
            observations.writes[1],
            vec![FakeWriteContent::Text("fallback".into())]
        );
        assert_eq!(observations.verified_formats, vec![ContentFormatKind::Text]);
    }

    #[test]
    fn unverified_mixed_fallback_fails_without_a_third_write() {
        let (backend, observations) = FakeClipboard::with_write_behavior(
            Vec::new(),
            vec![Ok(()), Ok(())],
            vec![true, false, false],
        );
        let clipboard = SystemClipboard::from_backend(backend);

        assert!(matches!(
            clipboard.write_item(&item(
                "fallback",
                Some(clipboard_image(vec![10, 20, 30, 40], 1, 1)),
            )),
            Err(SystemClipboardError::Verification)
        ));
        let observations = observations.lock().unwrap();
        assert_eq!(observations.writes.len(), 2);
        assert_eq!(
            observations.verified_formats,
            vec![
                ContentFormatKind::Image,
                ContentFormatKind::Text,
                ContentFormatKind::Text,
            ]
        );
    }

    #[test]
    fn unverified_text_write_fails_without_a_second_write() {
        let (backend, observations) =
            FakeClipboard::with_write_behavior(Vec::new(), Vec::new(), vec![false]);
        let clipboard = SystemClipboard::from_backend(backend);

        assert!(matches!(
            clipboard.write_item(&item("hello", None)),
            Err(SystemClipboardError::Verification)
        ));
        assert_eq!(observations.lock().unwrap().writes.len(), 1);
    }

    #[test]
    fn failed_image_write_does_not_use_empty_text_as_fallback() {
        let (backend, observations) = FakeClipboard::with_write_behavior(
            Vec::new(),
            vec![Err(SystemClipboardError::Write)],
            Vec::new(),
        );
        let clipboard = SystemClipboard::from_backend(backend);

        assert!(matches!(
            clipboard.write_item(&item("", Some(clipboard_image(vec![1, 2, 3, 4], 1, 1)),)),
            Err(SystemClipboardError::Write)
        ));
        assert_eq!(observations.lock().unwrap().writes.len(), 1);
    }

    #[test]
    fn empty_item_is_rejected_without_a_write() {
        let (backend, observations) =
            FakeClipboard::with_write_behavior(Vec::new(), Vec::new(), Vec::new());
        let clipboard = SystemClipboard::from_backend(backend);

        assert!(matches!(
            clipboard.write_item(&item("", None)),
            Err(SystemClipboardError::EmptyContent)
        ));
        assert!(observations.lock().unwrap().writes.is_empty());
    }
}
