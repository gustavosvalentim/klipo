use std::vec::Vec;

use tauri::image::Image;
use tauri::{AppHandle, State};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::clipboard::{ClipboardEventsEmitter, ClipboardImage, ClipboardItem};
use crate::input::simulate_paste_input;
use crate::state::AppState;
use crate::window::{get_main_window, restore_focused_window};
use crate::{settings::ShortcutSettings, shortcuts};

#[tauri::command]
pub fn fetch_clipboard(state: State<'_, AppState>) -> Vec<ClipboardItem> {
    println!("Fetch clipboard");

    state.clipboard.list().unwrap_or_default()
}

#[tauri::command]
pub fn get_shortcuts(state: State<'_, AppState>) -> Result<ShortcutSettings, String> {
    state
        .shortcuts
        .lock()
        .map(|settings| settings.clone())
        .map_err(|_| "Shortcut settings are unavailable".into())
}

#[tauri::command]
pub fn save_shortcuts(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: ShortcutSettings,
) -> Result<ShortcutSettings, String> {
    settings.validate()?;

    let mut active_shortcuts = state
        .shortcuts
        .lock()
        .map_err(|_| "Shortcut settings are unavailable")?;

    let previous = active_shortcuts.clone();

    shortcuts::replace_global_shortcuts(&app, &previous, &settings)?;

    let path = shortcuts::settings_path(&app).map_err(|error| error.to_string())?;
    if let Err(error) = crate::settings::save(&path, &settings) {
        let _ = shortcuts::replace_global_shortcuts(&app, &settings, &previous);
        return Err(format!("Could not save shortcut settings: {error}"));
    }

    *active_shortcuts = settings.clone();

    Ok(settings)
}

#[tauri::command]
pub fn clear(app: AppHandle, state: State<'_, AppState>) {
    if let Err(e) = state.clipboard.clear() {
        println!("Failed to clear clipboard history: {e}");
    }

    if let Err(e) = app.emit_clipboard_changed() {
        println!("Failed to emit clipboard changed event: {e}");
    }
}

/// Outcome of a paste decision: whether to continue with the paste flow or abort.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PasteOutcome {
    Continue,
    Abort,
}

/// Outcome of writing the new first item to the clipboard after deletion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FirstItemReplacementOutcome {
    Written,
    NoItem,
}

/// Write the new first item to the system clipboard after the previous first
/// item was deleted.
///
/// Returns `FirstItemReplacementOutcome::Written` after successfully writing
/// the item's primary content (or fallback text if image writing fails), or
/// `FirstItemReplacementOutcome::NoItem` if no item remains or nothing was
/// written.
///
/// The `write_image` and `write_text` closures allow testing the decision logic
/// without a real system clipboard.
pub(crate) fn perform_first_item_replacement(
    item: Option<&ClipboardItem>,
    write_image: impl FnOnce(&ClipboardImage) -> bool,
    write_text: impl FnOnce(&str) -> bool,
) -> FirstItemReplacementOutcome {
    let Some(item) = item else {
        return FirstItemReplacementOutcome::NoItem;
    };

    let wrote = match item.image.as_ref() {
        Some(image) => write_image(image) || (!item.text.is_empty() && write_text(&item.text)),
        None => !item.text.is_empty() && write_text(&item.text),
    };

    if wrote {
        FirstItemReplacementOutcome::Written
    } else {
        FirstItemReplacementOutcome::NoItem
    }
}

/// Decide whether and what clipboard content to write for a paste action.
///
/// Returns `PasteOutcome::Continue` after successfully writing content to the
/// clipboard, or `PasteOutcome::Abort` if nothing could be written.
///
/// The `write_image` and `write_text` closures allow testing the decision logic
/// without a real system clipboard.
pub(crate) fn perform_paste_decision(
    item: &ClipboardItem,
    write_image: impl FnOnce(&ClipboardImage) -> bool,
    write_text: impl FnOnce(&str) -> bool,
) -> PasteOutcome {
    let wrote = match item.image.as_ref() {
        Some(image) => write_image(image) || (!item.text.is_empty() && write_text(&item.text)),
        None => !item.text.is_empty() && write_text(&item.text),
    };
    if wrote {
        PasteOutcome::Continue
    } else {
        PasteOutcome::Abort
    }
}

fn perform_paste_flow(app: &AppHandle, state: &AppState) {
    if let Some(window) = get_main_window(app) {
        if window.hide().is_err() {
            println!("Failed to hide window");
        }
    }

    if restore_focused_window(state).is_err() {
        println!("Failed to restore focus");
        return;
    }

    let Ok(mut guard) = state.input.enigo.lock() else {
        println!("Failed to lock input state");
        return;
    };

    let Some(enigo) = guard.as_mut() else {
        println!("Failed to get enigo");
        return;
    };

    let _ = simulate_paste_input(enigo);
}

#[tauri::command]
pub fn paste(app: AppHandle, state: State<'_, AppState>, hash: &str) {
    let Some(item) = state.clipboard.get_by_hash(hash) else {
        return;
    };

    let write_image = |image: &ClipboardImage| -> bool {
        let img = Image::new_owned(image.rgba.clone(), image.width, image.height);
        app.clipboard().write_image(&img).is_ok()
    };
    let write_text = |text: &str| -> bool { app.clipboard().write_text(text).is_ok() };

    if perform_paste_decision(&item, write_image, write_text) == PasteOutcome::Continue {
        let _ = state.clipboard.move_to_top_by_hash(hash);
        perform_paste_flow(&app, &state);
    }
}

#[tauri::command]
pub fn quit(app: AppHandle) {
    let Some(window) = get_main_window(&app) else {
        println!("Failed to get main window");
        return;
    };

    if let Err(e) = window.close() {
        println!("Failed to close window: {e}");
    }
}

#[tauri::command]
pub fn close(app: AppHandle, state: State<'_, AppState>) {
    let Some(window) = get_main_window(&app) else {
        println!("Failed to get main window");
        return;
    };

    if let Err(e) = window.hide() {
        println!("Failed to hide window: {e}");
    }

    if let Err(e) = restore_focused_window(&state) {
        println!("Failed to restore focus: {e}");
    }
}

#[tauri::command]
pub fn delete_item(app: AppHandle, state: State<'_, AppState>, hash: &str) {
    if hash.is_empty() {
        return;
    }

    let Ok(item_idx) = state.clipboard.delete_by_hash(hash) else {
        println!("Failed to delete item from clipboard history");
        return;
    };

    if let Err(e) = app.emit_clipboard_changed() {
        println!("Failed to emit clipboard changed event: {e}");
    }

    if item_idx == 0 {
        let write_image = |image: &ClipboardImage| -> bool {
            let img = Image::new_owned(image.rgba.clone(), image.width, image.height);
            if app.clipboard().write_image(&img).is_ok() {
                true
            } else {
                println!("Failed to write image to clipboard");
                false
            }
        };
        let write_text = |text: &str| -> bool {
            if app.clipboard().write_text(text).is_ok() {
                true
            } else {
                println!("Failed to write text to clipboard");
                false
            }
        };
        perform_first_item_replacement(state.clipboard.first().as_ref(), write_image, write_text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::ClipboardImage;

    fn image(byte: u8) -> ClipboardImage {
        ClipboardImage::from_rgba(vec![byte; 4], 1, 1).unwrap()
    }

    #[test]
    fn decision_image_write_success_continues() {
        let item = ClipboardItem {
            text: String::new(),
            hash: "image:abc".into(),
            image: Some(image(0x42)),
            preview: None,
        };

        let outcome = perform_paste_decision(&item, |_| true, |_| false);
        assert_eq!(outcome, PasteOutcome::Continue);
    }

    #[test]
    fn decision_image_write_failure_with_fallback_continues() {
        let item = ClipboardItem {
            text: "alt text".into(),
            hash: "image:abc".into(),
            image: Some(image(0x42)),
            preview: None,
        };

        let outcome = perform_paste_decision(&item, |_| false, |_| true);
        assert_eq!(outcome, PasteOutcome::Continue);
    }

    #[test]
    fn decision_image_write_failure_no_fallback_aborts() {
        let item = ClipboardItem {
            text: String::new(),
            hash: "image:abc".into(),
            image: Some(image(0x42)),
            preview: None,
        };

        let outcome = perform_paste_decision(&item, |_| false, |_| false);
        assert_eq!(outcome, PasteOutcome::Abort);
    }

    #[test]
    fn decision_text_write_success_continues() {
        let item = ClipboardItem {
            text: "hello".into(),
            hash: "text:def".into(),
            image: None,
            preview: None,
        };

        let outcome = perform_paste_decision(&item, |_| false, |_| true);
        assert_eq!(outcome, PasteOutcome::Continue);
    }

    #[test]
    fn decision_text_write_failure_aborts() {
        let item = ClipboardItem {
            text: "hello".into(),
            hash: "text:def".into(),
            image: None,
            preview: None,
        };

        let outcome = perform_paste_decision(&item, |_| false, |_| false);
        assert_eq!(outcome, PasteOutcome::Abort);
    }

    #[test]
    fn decision_empty_item_aborts() {
        let item = ClipboardItem {
            text: String::new(),
            hash: "empty".into(),
            image: None,
            preview: None,
        };

        let outcome = perform_paste_decision(&item, |_| false, |_| false);
        assert_eq!(outcome, PasteOutcome::Abort);
    }

    #[test]
    fn decision_image_success_does_not_fall_back_to_text() {
        let mut text_written = false;
        let item = ClipboardItem {
            text: "fallback".into(),
            hash: "image:abc".into(),
            image: Some(image(0x42)),
            preview: None,
        };

        let outcome = perform_paste_decision(
            &item,
            |_| true,
            |_| {
                text_written = true;
                true
            },
        );

        // Image success should not call text write at all
        assert_eq!(outcome, PasteOutcome::Continue);
        assert!(
            !text_written,
            "text write should not be called when image succeeds"
        );
    }

    // --- first-item-replacement tests ---

    #[test]
    fn replacement_image_success_writes_image() {
        let item = ClipboardItem {
            text: "fallback".into(),
            hash: "image:abc".into(),
            image: Some(image(0x42)),
            preview: None,
        };

        let outcome = perform_first_item_replacement(Some(&item), |_| true, |_| false);
        assert_eq!(outcome, FirstItemReplacementOutcome::Written);
    }

    #[test]
    fn replacement_image_failure_falls_back_to_text() {
        let item = ClipboardItem {
            text: "alt text".into(),
            hash: "image:abc".into(),
            image: Some(image(0x42)),
            preview: None,
        };

        let outcome = perform_first_item_replacement(Some(&item), |_| false, |_| true);
        assert_eq!(outcome, FirstItemReplacementOutcome::Written);
    }

    #[test]
    fn replacement_image_failure_no_fallback_returns_no_item() {
        let item = ClipboardItem {
            text: String::new(),
            hash: "image:abc".into(),
            image: Some(image(0x42)),
            preview: None,
        };

        let outcome = perform_first_item_replacement(Some(&item), |_| false, |_| false);
        assert_eq!(outcome, FirstItemReplacementOutcome::NoItem);
    }

    #[test]
    fn replacement_text_success_writes_text() {
        let item = ClipboardItem {
            text: "hello".into(),
            hash: "text:def".into(),
            image: None,
            preview: None,
        };

        let outcome = perform_first_item_replacement(Some(&item), |_| false, |_| true);
        assert_eq!(outcome, FirstItemReplacementOutcome::Written);
    }

    #[test]
    fn replacement_text_failure_returns_no_item() {
        let item = ClipboardItem {
            text: "hello".into(),
            hash: "text:def".into(),
            image: None,
            preview: None,
        };

        let outcome = perform_first_item_replacement(Some(&item), |_| false, |_| false);
        assert_eq!(outcome, FirstItemReplacementOutcome::NoItem);
    }

    #[test]
    fn replacement_no_item_returns_no_item() {
        let outcome = perform_first_item_replacement(None, |_| true, |_| true);
        assert_eq!(outcome, FirstItemReplacementOutcome::NoItem);
    }

    #[test]
    fn replacement_image_success_does_not_fall_back_to_text() {
        let mut text_written = false;
        let item = ClipboardItem {
            text: "fallback".into(),
            hash: "image:abc".into(),
            image: Some(image(0x42)),
            preview: None,
        };

        let outcome = perform_first_item_replacement(
            Some(&item),
            |_| true,
            |_| {
                text_written = true;
                true
            },
        );

        assert_eq!(outcome, FirstItemReplacementOutcome::Written);
        assert!(
            !text_written,
            "text write should not be called when image succeeds"
        );
    }
}
