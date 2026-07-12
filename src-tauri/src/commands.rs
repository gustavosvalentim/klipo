use std::vec::Vec;

use tauri::Emitter;
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::clipboard::{ClipboardItem, ClipboardManager, InMemoryClipboardHistory};
use crate::paste;
use crate::window::get_main_window;

#[tauri::command]
pub fn list_clipboard_items(
    history: tauri::State<'_, InMemoryClipboardHistory>,
) -> Vec<ClipboardItem> {
    match history.list() {
        Ok(items) => items,
        Err(e) => {
            println!("Failed to list clipboard history: {e}");
            Vec::new()
        }
    }
}

#[tauri::command]
pub fn clear_clipboard_items(history: tauri::State<'_, InMemoryClipboardHistory>) {
    match history.clear() {
        Ok(_) => {}
        Err(e) => {
            println!("Failed to clear clipboard history: {e}");
        }
    }
}

#[tauri::command]
pub fn paste_from_selection(app: tauri::AppHandle, text: &str) {
    match paste::paste(&app, text) {
        Ok(_) => {
            app.emit("clipboard-changed", text).unwrap();
        }
        Err(e) => {
            println!("Failed to paste from selection: {e}");
        }
    }
}

#[tauri::command]
pub fn quit_clipbox(app: tauri::AppHandle) {
    match get_main_window(&app) {
        Some(window) => {
            let _ = window.close();
        }
        None => println!("Failed to get main window"),
    };
}

#[tauri::command]
pub fn hide_clipbox(app: tauri::AppHandle) {
    match get_main_window(&app) {
        Some(window) => {
            let _ = window.hide();
        }
        None => println!("Failed to get main window"),
    };
}

#[tauri::command]
pub fn delete_item(
    app: tauri::AppHandle,
    history: tauri::State<'_, InMemoryClipboardHistory>,
    text: String,
) {
    let Ok(item_idx) = history.delete(&text) else {
        println!("Failed to delete item from clipboard history");
        return;
    };

    let item_idx = item_idx as u32;

    if item_idx == 0 {
        if let Ok(item) = history.first() {
            app.clipboard().write_text(item.text).unwrap();
        }
    }

    app.emit("clipboard-changed", text).unwrap();
}
