use std::vec::Vec;

use log::{debug, error};
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::clipboard::{ClipboardEventsEmitter, ClipboardItem, SystemClipboard};
use crate::desktop::{DesktopCapabilities, DesktopCapability};
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
pub fn get_capabilities(state: State<'_, AppState>) -> Result<DesktopCapabilities, String> {
    state.capabilities()
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
    let native_shortcuts_available = state.capability_is_available(DesktopCapability::Shortcut);

    save_shortcut_transaction(
        native_shortcuts_available,
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
    replace_runtime: bool,
    previous: &ShortcutSettings,
    next: &ShortcutSettings,
    prepare_persistence: impl FnOnce() -> Result<Destination, String>,
    mut replace: impl FnMut(&ShortcutSettings, &ShortcutSettings) -> Result<(), String>,
    persist: impl FnOnce(&Destination, &ShortcutSettings) -> Result<(), String>,
) -> Result<(), String> {
    let destination = prepare_persistence()?;

    if replace_runtime {
        replace(previous, next)?;
    }

    match persist(&destination, next) {
        Ok(()) => Ok(()),
        Err(error_value) if !replace_runtime => {
            Err(format!("Could not save shortcut settings: {error_value}"))
        }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum PasteOutcome {
    Pasted,
    CopiedForManualPaste,
    ClipboardWriteFailed,
}

trait PasteOperations {
    fn item_for_hash(&self, hash: &str) -> Option<ClipboardItem>;
    fn write_item(&self, item: &ClipboardItem) -> Result<(), String>;
    fn move_to_top(&self, hash: &str) -> Result<(), String>;
    fn restore_target(&self) -> Result<(), String>;
    fn hide_picker(&self) -> Result<(), String>;
    fn simulate_input(&self) -> Result<(), String>;
}

struct AppPasteOperations<'a> {
    app: &'a AppHandle,
    state: &'a AppState,
}

impl PasteOperations for AppPasteOperations<'_> {
    fn item_for_hash(&self, hash: &str) -> Option<ClipboardItem> {
        self.state.clipboard.get_by_hash(hash)
    }

    fn write_item(&self, item: &ClipboardItem) -> Result<(), String> {
        let system_clipboard = self
            .state
            .system_clipboard
            .as_ref()
            .ok_or_else(|| "Clipboard write is unavailable".to_owned())?;

        write_to_clipboard(system_clipboard, item).map_err(|error| error.to_string())
    }

    fn move_to_top(&self, hash: &str) -> Result<(), String> {
        self.state
            .clipboard
            .move_to_top_by_hash(hash)
            .map_err(|error| error.to_string())
    }

    fn restore_target(&self) -> Result<(), String> {
        restore_focused_window(self.state).map_err(|error| error.to_string())
    }

    fn hide_picker(&self) -> Result<(), String> {
        let window = get_main_window(self.app)
            .ok_or_else(|| String::from("Main picker window unavailable"))?;
        window.hide().map_err(|error| error.to_string())
    }

    fn simulate_input(&self) -> Result<(), String> {
        let mut guard = self
            .state
            .input
            .enigo
            .lock()
            .map_err(|_| "Input state is unavailable".to_string())?;
        let enigo = guard
            .as_mut()
            .ok_or_else(|| "Input simulation is unavailable".to_string())?;

        simulate_paste_input(enigo).map_err(|error| error.to_string())
    }
}

fn paste_with(operations: &impl PasteOperations, hash: &str) -> PasteOutcome {
    let Some(item) = operations.item_for_hash(hash) else {
        return PasteOutcome::ClipboardWriteFailed;
    };

    if let Err(error_value) = operations.write_item(&item) {
        error!(error:% = error_value; "Failed to write item to clipboard");
        return PasteOutcome::ClipboardWriteFailed;
    }

    if let Err(error_value) = operations.move_to_top(hash) {
        error!(error:% = error_value; "Failed to move copied item to the top");
        return PasteOutcome::CopiedForManualPaste;
    }

    if let Err(error_value) = operations.restore_target() {
        error!(error:% = error_value; "Failed to restore paste target");
        return PasteOutcome::CopiedForManualPaste;
    }

    if let Err(error_value) = operations.hide_picker() {
        error!(error:% = error_value; "Failed to hide picker before automatic paste");
        return PasteOutcome::CopiedForManualPaste;
    }

    if let Err(error_value) = operations.simulate_input() {
        error!(error:% = error_value; "Failed to simulate paste input");
        return PasteOutcome::CopiedForManualPaste;
    }

    PasteOutcome::Pasted
}

#[tauri::command]
pub fn paste(app: AppHandle, state: State<'_, AppState>, hash: &str) -> PasteOutcome {
    let operations = AppPasteOperations {
        app: &app,
        state: &state,
    };

    paste_with(&operations, hash)
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
            let Some(system_clipboard) = state.system_clipboard.as_ref() else {
                error!(capability = "clipboard_write", failure_category = "adapter_unavailable"; "Clipboard write is unavailable");
                return;
            };

            if let Err(error_value) = write_to_clipboard(system_clipboard, &item) {
                error!(error:debug = error_value; "Failed to write first item to clipboard");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        paste_with, save_shortcut_transaction, should_emit_clear_event, PasteOperations,
        PasteOutcome,
    };
    use crate::clipboard::ClipboardItem;
    use crate::settings::ShortcutSettings;
    use crate::storage::ClipboardError;

    struct FakePasteOperations {
        item: Option<ClipboardItem>,
        write_result: Result<(), String>,
        reorder_result: Result<(), String>,
        restore_result: Result<(), String>,
        hide_result: Result<(), String>,
        input_result: Result<(), String>,
        events: std::cell::RefCell<Vec<&'static str>>,
    }

    impl FakePasteOperations {
        fn successful() -> Self {
            Self {
                item: Some(ClipboardItem {
                    text: "copied text".into(),
                    hash: "text:known".into(),
                    image: None,
                    preview: None,
                }),
                write_result: Ok(()),
                reorder_result: Ok(()),
                restore_result: Ok(()),
                hide_result: Ok(()),
                input_result: Ok(()),
                events: std::cell::RefCell::new(Vec::new()),
            }
        }

        fn failed(message: &str) -> Result<(), String> {
            Err(message.into())
        }

        fn events(&self) -> Vec<&'static str> {
            self.events.borrow().clone()
        }
    }

    impl PasteOperations for FakePasteOperations {
        fn item_for_hash(&self, _hash: &str) -> Option<ClipboardItem> {
            self.item.clone()
        }

        fn write_item(&self, _item: &ClipboardItem) -> Result<(), String> {
            self.events.borrow_mut().push("write");
            self.write_result.clone()
        }

        fn move_to_top(&self, _hash: &str) -> Result<(), String> {
            self.events.borrow_mut().push("reorder");
            self.reorder_result.clone()
        }

        fn restore_target(&self) -> Result<(), String> {
            self.events.borrow_mut().push("restore");
            self.restore_result.clone()
        }

        fn hide_picker(&self) -> Result<(), String> {
            self.events.borrow_mut().push("hide");
            self.hide_result.clone()
        }

        fn simulate_input(&self) -> Result<(), String> {
            self.events.borrow_mut().push("input");
            self.input_result.clone()
        }
    }

    fn assert_manual_paste_after_input_failure(error: &str) {
        let mut operations = FakePasteOperations::successful();
        operations.input_result = FakePasteOperations::failed(error);

        assert_eq!(
            paste_with(&operations, "text:known"),
            PasteOutcome::CopiedForManualPaste
        );
        assert_eq!(
            operations.events(),
            ["write", "reorder", "restore", "hide", "input"]
        );
    }

    #[test]
    fn clear_event_is_emitted_only_after_a_committed_clear() {
        assert!(should_emit_clear_event(&Ok(())));
        assert!(!should_emit_clear_event(&Err(ClipboardError::ItemNotFound)));
    }

    #[test]
    fn persists_shortcut_settings_without_touching_an_unavailable_plugin() {
        let mut native_calls = 0;
        let mut persisted = false;

        let result = save_shortcut_transaction(
            false,
            &ShortcutSettings::default(),
            &ShortcutSettings::default(),
            || Ok(()),
            |_, _| {
                native_calls += 1;
                Ok(())
            },
            |_, _| {
                persisted = true;
                Ok(())
            },
        );

        assert!(result.is_ok());
        assert_eq!(native_calls, 0);
        assert!(persisted);
    }

    #[test]
    fn persistence_failure_restores_the_previous_runtime_binding() {
        let previous = ShortcutSettings::default();
        let mut next = ShortcutSettings::default();
        next.open_klipo = "SUPER+ALT+KeyK".into();
        let mut replacements = Vec::new();

        let error_value = save_shortcut_transaction(
            true,
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
            true,
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
            true,
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

    #[test]
    fn paste_outcomes_serialize_as_the_frontend_contract() {
        assert_eq!(
            serde_json::to_value(PasteOutcome::Pasted).unwrap(),
            serde_json::json!("Pasted")
        );
        assert_eq!(
            serde_json::to_value(PasteOutcome::CopiedForManualPaste).unwrap(),
            serde_json::json!("CopiedForManualPaste")
        );
        assert_eq!(
            serde_json::to_value(PasteOutcome::ClipboardWriteFailed).unwrap(),
            serde_json::json!("ClipboardWriteFailed")
        );
    }

    #[test]
    fn unknown_hash_does_not_change_history_or_picker() {
        let mut operations = FakePasteOperations::successful();
        operations.item = None;

        assert_eq!(
            paste_with(&operations, "text:unknown"),
            PasteOutcome::ClipboardWriteFailed
        );
        assert!(operations.events().is_empty());
    }

    #[test]
    fn clipboard_write_failure_does_not_change_history_or_picker() {
        let mut operations = FakePasteOperations::successful();
        operations.write_result = FakePasteOperations::failed("write");

        assert_eq!(
            paste_with(&operations, "text:known"),
            PasteOutcome::ClipboardWriteFailed
        );
        assert_eq!(operations.events(), ["write"]);
    }

    #[test]
    fn successful_paste_writes_reorders_restores_hides_then_inputs() {
        let operations = FakePasteOperations::successful();

        assert_eq!(paste_with(&operations, "text:known"), PasteOutcome::Pasted);
        assert_eq!(
            operations.events(),
            ["write", "reorder", "restore", "hide", "input"]
        );
    }

    #[test]
    fn reorder_failure_keeps_picker_open_for_manual_paste() {
        let mut operations = FakePasteOperations::successful();
        operations.reorder_result = FakePasteOperations::failed("reorder");

        assert_eq!(
            paste_with(&operations, "text:known"),
            PasteOutcome::CopiedForManualPaste
        );
        assert_eq!(operations.events(), ["write", "reorder"]);
    }

    #[test]
    fn unavailable_target_keeps_picker_open_for_manual_paste() {
        let mut operations = FakePasteOperations::successful();
        operations.restore_result = FakePasteOperations::failed("target unavailable");

        assert_eq!(
            paste_with(&operations, "text:known"),
            PasteOutcome::CopiedForManualPaste
        );
        assert_eq!(operations.events(), ["write", "reorder", "restore"]);
    }

    #[test]
    fn hide_failure_does_not_attempt_input_or_report_success() {
        let mut operations = FakePasteOperations::successful();
        operations.hide_result = FakePasteOperations::failed("hide");

        assert_eq!(
            paste_with(&operations, "text:known"),
            PasteOutcome::CopiedForManualPaste
        );
        assert_eq!(operations.events(), ["write", "reorder", "restore", "hide"]);
    }

    #[test]
    fn unavailable_input_keeps_picker_open_for_manual_paste() {
        assert_manual_paste_after_input_failure("input unavailable");
    }

    #[test]
    fn modifier_press_failure_is_not_a_successful_paste() {
        assert_manual_paste_after_input_failure("modifier press");
    }

    #[test]
    fn v_click_failure_is_not_a_successful_paste() {
        assert_manual_paste_after_input_failure("v click");
    }

    #[test]
    fn modifier_release_failure_is_not_a_successful_paste() {
        assert_manual_paste_after_input_failure("modifier release");
    }

    #[test]
    fn click_and_release_failure_is_not_a_successful_paste() {
        assert_manual_paste_after_input_failure("v click and modifier release");
    }
}
