use std::vec::Vec;

use log::{debug, error, warn};
use tauri::image::Image;
use tauri::{AppHandle, State};
use tauri_plugin_clipboard_manager::{
    ClipboardExt, Error as ClipboardError, Result as ClipboardResult,
};

use crate::clipboard::{ClipboardEventsEmitter, ClipboardImage, ClipboardItem};
use crate::input::simulate_paste_input;
use crate::state::AppState;
use crate::window::{get_main_window, restore_focused_window};
use crate::{settings::ShortcutSettings, shortcuts};

#[tauri::command]
pub fn fetch_clipboard(state: State<'_, AppState>) -> Vec<ClipboardItem> {
    let items = state
        .clipboard
        .list_for_display()
        .unwrap_or_else(|error_value| {
            error!(error:debug = error_value; "Failed to fetch clipboard history");
            Vec::new()
        });
    debug!(item_count = items.len(); "Fetched clipboard history");
    items
}

#[tauri::command]
pub fn log_frontend_error(context: String, error: String) {
    error!(context:% = context, error:% = error; "Frontend error");
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
    if let Err(error_value) = state.clipboard.clear() {
        error!(error:debug = error_value; "Failed to clear clipboard history");
    }

    if let Err(error_value) = app.emit_clipboard_changed() {
        error!(error:debug = error_value; "Failed to emit clipboard changed event");
    }
}

const EMPTY_TEXT_ERROR: &str = "No text to write to clipboard";

fn validate_text_to_clipboard(text: &str) -> ClipboardResult<()> {
    if text.is_empty() {
        return Err(ClipboardError::Clipboard(EMPTY_TEXT_ERROR.into()));
    }

    Ok(())
}

/// Write text to the system clipboard. Empty text is treated as a failed write.
fn write_text_to_clipboard(app: &AppHandle, text: &str) -> ClipboardResult<()> {
    validate_text_to_clipboard(text)?;
    app.clipboard().write_text(text)
}

/// Write an image to the system clipboard.
fn write_image_to_clipboard(
    app: &AppHandle,
    image: &ClipboardImage,
    fallback: &str,
) -> ClipboardResult<()> {
    let img = Image::new_owned(image.rgba.clone(), image.width, image.height);

    app.clipboard().write_image(&img).or_else(|error_value| {
        warn!(error:debug = error_value; "Failed to write image to clipboard; trying text fallback");
        write_text_to_clipboard(app, fallback)
    })
}

/// Write a clipboard item's content to the system clipboard.
///
/// Writes the item's image if present, falling back to the item's text if
/// there is no image or image writing fails. Clipboard writes replace the
/// whole contents, so only one of the two ends up written.
pub(crate) fn write_to_clipboard(app: &AppHandle, item: &ClipboardItem) -> ClipboardResult<()> {
    match item.image.as_ref() {
        Some(image) => write_image_to_clipboard(app, image, &item.text),
        None => write_text_to_clipboard(app, &item.text),
    }
}

#[tauri::command]
pub fn paste(app: AppHandle, state: State<'_, AppState>, hash: &str) {
    let Some(item) = state.clipboard.get_by_hash(hash) else {
        return;
    };

    if let Err(error_value) = write_to_clipboard(&app, &item) {
        error!(error:debug = error_value; "Failed to write item to clipboard");
        return;
    }

    if let Err(error_value) = state.clipboard.move_to_top_by_hash(hash) {
        error!(error:debug = error_value; "Failed to move pasted item to the top");
    }

    if let Some(window) = get_main_window(&app) {
        if let Err(error_value) = window.hide() {
            error!(error:debug = error_value; "Failed to hide window");
        }
    }

    if let Err(error_value) = restore_focused_window(&state) {
        error!(error:debug = error_value; "Failed to restore focus");
        return;
    }

    let Ok(mut guard) = state.input.enigo.lock() else {
        error!("Failed to lock input state");
        return;
    };

    let Some(enigo) = guard.as_mut() else {
        error!("Failed to get enigo");
        return;
    };

    if let Err(error_value) = simulate_paste_input(enigo) {
        error!(error:debug = error_value; "Failed to simulate paste input");
    }
}

#[tauri::command]
pub fn quit(app: AppHandle) {
    let Some(window) = get_main_window(&app) else {
        error!("Failed to get main window");
        return;
    };

    if let Err(error_value) = window.close() {
        error!(error:debug = error_value; "Failed to close window");
    }
}

#[tauri::command]
pub fn close(app: AppHandle, state: State<'_, AppState>) {
    let Some(window) = get_main_window(&app) else {
        error!("Failed to get main window");
        return;
    };

    if let Err(error_value) = window.hide() {
        error!(error:debug = error_value; "Failed to hide window");
    }

    if let Err(error_value) = restore_focused_window(&state) {
        error!(error:debug = error_value; "Failed to restore focus");
    }
}

#[tauri::command]
pub fn delete_item(app: AppHandle, state: State<'_, AppState>, hash: &str) {
    if hash.is_empty() {
        return;
    }

    let Ok(item_idx) = state.clipboard.delete_by_hash(hash) else {
        error!("Failed to delete item from clipboard history");
        return;
    };

    if let Err(error_value) = app.emit_clipboard_changed() {
        error!(error:debug = error_value; "Failed to emit clipboard changed event");
    }

    if item_idx == 0 {
        if let Some(item) = state.clipboard.first() {
            if let Err(error_value) = write_to_clipboard(&app, &item) {
                error!(error:debug = error_value; "Failed to write first item to clipboard");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{validate_text_to_clipboard, ClipboardError, EMPTY_TEXT_ERROR};

    #[test]
    fn empty_text_is_rejected_as_a_clipboard_write() {
        assert!(matches!(
            validate_text_to_clipboard(""),
            Err(ClipboardError::Clipboard(message)) if message == EMPTY_TEXT_ERROR
        ));
    }
}
