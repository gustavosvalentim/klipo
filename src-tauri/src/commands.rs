use std::vec::Vec;

use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::clipboard::{ClipboardEventsEmitter, ClipboardItem, ClipboardStore};
use crate::paste;
use crate::window::get_main_window;

#[tauri::command]
pub fn list_clipboard_items(history: tauri::State<'_, ClipboardStore>) -> Vec<ClipboardItem> {
    match history.list() {
        Ok(items) => items,
        Err(e) => {
            println!("Failed to list clipboard history: {e}");
            Vec::new()
        }
    }
}

#[tauri::command]
pub fn clear_clipboard_items(history: tauri::State<'_, ClipboardStore>) {
    if let Err(e) = history.clear() {
        println!("Failed to clear clipboard history: {e}");
    }
}

#[tauri::command]
pub fn paste_from_selection(app: tauri::AppHandle, text: &str) {
    if let Err(e) = paste::paste(&app, text) {
        println!("Failed to paste from selection: {e}");
    }
}

#[tauri::command]
pub fn quit_clipbox(app: tauri::AppHandle) {
    let Some(window) = get_main_window(&app) else {
        println!("Failed to get main window");
        return;
    };

    let _ = window.close();
}

#[tauri::command]
pub fn hide_clipbox(app: tauri::AppHandle) {
    let Some(window) = get_main_window(&app) else {
        println!("Failed to get main window");
        return;
    };

    let _ = window.hide();
}

#[tauri::command]
pub fn delete_item(app: tauri::AppHandle, history: tauri::State<'_, ClipboardStore>, text: String) {
    let Ok(item_idx) = history.delete(&text) else {
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
    } else {
        if let Err(e) = app.clipboard().write_text(" ") {
            println!("Failed to write text to clipboard: {e}");
        }
    }
}
