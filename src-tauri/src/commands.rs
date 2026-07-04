use std::vec::Vec;

use tauri::{Manager};

use crate::clipboard::{ClipboardItem, ClipboardManager, InMemoryClipboardHistory};
use crate::paste::PasteService;

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
pub fn paste_from_selection(app: tauri::AppHandle, text: String) {
    PasteService::new(app).paste_from_selection(text);
}

#[tauri::command]
pub fn quit_clipbox(app: tauri::AppHandle) {
    match app.get_webview_window("main") {
        Some(window) => {
            let _ = window.close();
        },
        None => println!("Failed to get main window"),
    };
}
