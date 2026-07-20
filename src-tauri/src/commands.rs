use std::vec::Vec;

use tauri::{AppHandle, State};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::clipboard::{ClipboardEventsEmitter, ClipboardItem, ClipboardStore};
use crate::input::InputState;
use crate::paste::{paste_from_selection, WindowManager};
use crate::window::get_main_window;

#[tauri::command]
pub fn fetch_clipboard(history: State<'_, ClipboardStore>) -> Vec<ClipboardItem> {
    println!("Fetch clipboard");

    history.list().unwrap_or_default()
}

#[tauri::command]
pub fn clear(app: AppHandle, history: State<'_, ClipboardStore>) {
    if let Err(e) = history.clear() {
        println!("Failed to clear clipboard history: {e}");
    }

    if let Err(e) = app.emit_clipboard_changed() {
        println!("Failed to emit clipboard changed event: {e}");
    }
}

#[tauri::command]
pub fn paste(
    app: AppHandle,
    history: State<'_, ClipboardStore>,
    paste: State<'_, WindowManager>,
    input: State<'_, InputState>,
    text: &str,
) {
    if let Err(e) = paste_from_selection(&app, &history, &paste, &input, text) {
        println!("Failed to paste from selection: {e}");
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
pub fn close(app: AppHandle, paste_target: State<'_, WindowManager>) {
    let Some(window) = get_main_window(&app) else {
        println!("Failed to get main window");
        return;
    };

    if let Err(e) = window.hide() {
        println!("Failed to hide window: {e}");
    }

    if let Err(e) = paste_target.restore_focus() {
        println!("Failed to restore focus: {e}");
    }
}

#[tauri::command]
pub fn delete_item(app: AppHandle, history: State<'_, ClipboardStore>, text: &str) {
    if text.is_empty() {
        return;
    }

    let Ok(item_idx) = history.delete(text) else {
        println!("Failed to delete item from clipboard history");
        return;
    };

    if let Err(e) = app.emit_clipboard_changed() {
        println!("Failed to emit clipboard changed event: {e}");
    }

    if item_idx == 0 {
        let Some(item) = history.first() else {
            return;
        };

        if let Err(e) = app.clipboard().write_text(item.text) {
            println!("Failed to write text to clipboard: {e}");
        }
    }
}
