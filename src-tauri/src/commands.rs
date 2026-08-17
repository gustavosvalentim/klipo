use std::vec::Vec;

use log::{debug, error};
use tauri::{AppHandle, State};

use crate::clipboard::{ClipboardEventsEmitter, ClipboardItem, SystemClipboard};
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

    save_shortcut_transaction(
        &previous,
        &settings,
        || shortcuts::settings_path(&app).map_err(|error| error.to_string()),
        |active, requested| shortcuts::replace_global_shortcuts(&app, active, requested),
        |path, requested| crate::settings::save(path, requested),
    )?;

    *active_shortcuts = settings.clone();

    Ok(settings)
}

fn save_shortcut_transaction<Destination>(
    previous: &ShortcutSettings,
    next: &ShortcutSettings,
    prepare_persistence: impl FnOnce() -> Result<Destination, String>,
    mut replace: impl FnMut(&ShortcutSettings, &ShortcutSettings) -> Result<(), String>,
    persist: impl FnOnce(&Destination, &ShortcutSettings) -> Result<(), String>,
) -> Result<(), String> {
    let destination = prepare_persistence()?;

    replace(previous, next)?;

    match persist(&destination, next) {
        Ok(()) => Ok(()),
        Err(error_value) => match replace(next, previous) {
            Ok(()) => Err(format!(
                "Could not save shortcut settings: {error_value}; restored the previous global shortcut binding"
            )),
            Err(rollback_error) => Err(format!(
                "Could not save shortcut settings: {error_value}; failed to restore the previous global shortcut binding ({rollback_error})"
            )),
        },
    }
}

#[tauri::command]
pub fn clear(app: AppHandle, state: State<'_, AppState>) {
    let clear_result = state.clipboard.clear();
    if should_emit_clear_event(&clear_result) {
        if let Err(error_value) = app.emit_clipboard_changed() {
            error!(error:debug = error_value; "Failed to emit clipboard changed event");
        }
    } else if let Err(error_value) = clear_result {
        error!(error:debug = error_value; "Failed to clear clipboard history");
    }
}

fn should_emit_clear_event(result: &Result<(), crate::storage::ClipboardError>) -> bool {
    result.is_ok()
}

/// Write and verify a clipboard item's content before any paste side effects.
pub(crate) fn write_to_clipboard(
    system_clipboard: &SystemClipboard,
    item: &ClipboardItem,
) -> Result<(), crate::clipboard::SystemClipboardError> {
    system_clipboard.write_item(item)
}

#[tauri::command]
pub fn paste(app: AppHandle, state: State<'_, AppState>, hash: &str) {
    let Some(item) = state.clipboard.get_by_hash(hash) else {
        return;
    };

    if let Err(error_value) = write_to_clipboard(&state.system_clipboard, &item) {
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
            if let Err(error_value) = write_to_clipboard(&state.system_clipboard, &item) {
                error!(error:debug = error_value; "Failed to write first item to clipboard");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{save_shortcut_transaction, should_emit_clear_event};
    use crate::settings::ShortcutSettings;
    use crate::storage::ClipboardError;

    #[test]
    fn clear_event_is_emitted_only_after_a_committed_clear() {
        assert!(should_emit_clear_event(&Ok(())));
        assert!(!should_emit_clear_event(&Err(ClipboardError::ItemNotFound)));
    }

    #[test]
    fn persistence_failure_restores_the_previous_runtime_binding() {
        let previous = ShortcutSettings::default();
        let mut next = ShortcutSettings::default();
        next.open_klipo = "SUPER+ALT+KeyK".into();
        let mut replacements = Vec::new();

        let error_value = save_shortcut_transaction(
            &previous,
            &next,
            || Ok(()),
            |active, requested| {
                replacements.push((active.open_klipo.clone(), requested.open_klipo.clone()));
                Ok(())
            },
            |_, _| Err("disk is unavailable".into()),
        )
        .expect_err("persistence failure rolls the binding back");

        assert!(error_value.contains("restored the previous"));
        assert_eq!(
            replacements,
            vec![
                ("SUPER+SHIFT+KeyV".into(), "SUPER+ALT+KeyK".into()),
                ("SUPER+ALT+KeyK".into(), "SUPER+SHIFT+KeyV".into()),
            ]
        );
    }

    #[test]
    fn failed_persistence_preparation_leaves_the_runtime_binding_unchanged() {
        let previous = ShortcutSettings::default();
        let mut next = ShortcutSettings::default();
        next.open_klipo = "SUPER+ALT+KeyK".into();
        let mut replacement_attempted = false;

        let error_value = save_shortcut_transaction(
            &previous,
            &next,
            || Err("settings directory is unavailable".into()),
            |_, _| {
                replacement_attempted = true;
                Ok(())
            },
            |_: &(), _| Ok(()),
        )
        .expect_err("preparation failure prevents runtime replacement");

        assert_eq!(error_value, "settings directory is unavailable");
        assert!(!replacement_attempted);
    }

    #[test]
    fn persistence_failure_reports_when_the_inverse_runtime_replacement_also_fails() {
        let previous = ShortcutSettings::default();
        let mut next = ShortcutSettings::default();
        next.open_klipo = "SUPER+ALT+KeyK".into();
        let mut replacements = Vec::new();

        let error_value = save_shortcut_transaction(
            &previous,
            &next,
            || Ok(()),
            |active, requested| {
                replacements.push((active.open_klipo.clone(), requested.open_klipo.clone()));
                if replacements.len() == 2 {
                    Err("X11 backend refused rollback".into())
                } else {
                    Ok(())
                }
            },
            |_, _| Err("disk is unavailable".into()),
        )
        .expect_err("a failed inverse replacement is returned to the caller");

        assert!(error_value.contains("disk is unavailable"));
        assert!(error_value.contains("X11 backend refused rollback"));
        assert_eq!(
            replacements,
            vec![
                ("SUPER+SHIFT+KeyV".into(), "SUPER+ALT+KeyK".into()),
                ("SUPER+ALT+KeyK".into(), "SUPER+SHIFT+KeyV".into()),
            ]
        );
    }
}
