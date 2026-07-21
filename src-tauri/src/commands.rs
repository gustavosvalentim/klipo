use std::vec::Vec;

use tauri::{AppHandle, State};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::clipboard::{ClipboardEventsEmitter, ClipboardItem};
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

#[tauri::command]
pub fn paste(app: AppHandle, state: State<'_, AppState>, text: &str) {
    if !state.clipboard.exists(text) {
        return;
    }

    if app.clipboard().write_text(text).is_err() {
        println!("Failed to write text to clipboard");
        return;
    }

    let _ = state.clipboard.move_to_top(text);

    if let Some(window) = get_main_window(&app) {
        if window.hide().is_err() {
            println!("Failed to hide window");
        }
    }

    if restore_focused_window(&state).is_err() {
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
pub fn delete_item(app: AppHandle, state: State<'_, AppState>, text: &str) {
    if text.is_empty() {
        return;
    }

    let Ok(item_idx) = state.clipboard.delete(text) else {
        println!("Failed to delete item from clipboard history");
        return;
    };

    if let Err(e) = app.emit_clipboard_changed() {
        println!("Failed to emit clipboard changed event: {e}");
    }

    if item_idx == 0 {
        let Some(item) = state.clipboard.first() else {
            return;
        };

        if let Err(e) = app.clipboard().write_text(item.text) {
            println!("Failed to write text to clipboard: {e}");
        }
    }
}
