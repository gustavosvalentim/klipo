use std::vec::Vec;

use log::{debug, error};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::clipboard::ClipboardEventsEmitter;
use crate::content::{HistorySummary, PasteContent};
use crate::desktop::{DesktopCapabilities, DesktopCapability};
use crate::state::AppState;
use crate::window::show_settings_window;
use crate::{settings::ShortcutSettings, shortcuts};

#[tauri::command]
pub fn fetch_clipboard(state: State<'_, AppState>) -> Result<Vec<HistorySummary>, String> {
    let items = state.clipboard.list_for_display().map_err(|error_value| {
        error!(error:debug = error_value; "Failed to fetch clipboard history");
        error_value.to_string()
    })?;
    debug!(item_count = items.len(); "Fetched clipboard history");
    Ok(items)
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

    let save_result = save_shortcut_transaction(
        native_shortcuts_available,
        &previous,
        &settings,
        || shortcuts::settings_path(&app).map_err(|error| error.to_string()),
        |active, requested| shortcuts::replace_global_shortcuts(&app, active, requested),
        |path, requested| crate::settings::save(path, requested),
    );
    if let Err(error_value) = save_result {
        let previous_binding_missing = native_shortcuts_available
            && previous.open_klipo != settings.open_klipo
            && !shortcuts::binding_is_registered(&app, &previous).unwrap_or(false);
        if previous_binding_missing {
            state.set_capability(
                DesktopCapability::Shortcut,
                crate::desktop::CapabilityStatus::unavailable(
                    crate::desktop::CapabilityUnavailableReason::InitializationFailed,
                ),
            );
        }
        return Err(error_value);
    }

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
    let runtime_change = replace_runtime && previous.open_klipo != next.open_klipo;

    if runtime_change {
        replace(previous, next)?;
    }

    match persist(&destination, next) {
        Ok(()) => Ok(()),
        Err(error_value) if !runtime_change => {
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
pub fn clear(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    state
        .clipboard
        .clear()
        .map_err(|error_value| error_value.to_string())?;
    if let Err(error_value) = app.emit_clipboard_changed() {
        error!(error:debug = error_value; "Failed to emit clipboard changed event");
    }
    Ok(())
}

#[tauri::command]
pub fn paste(
    app: AppHandle,
    state: State<'_, AppState>,
    hash: &str,
) -> Result<crate::picker::PasteOutcome, String> {
    crate::picker::paste(&app, &state, hash)
}

#[tauri::command]
pub fn quit(app: AppHandle) {
    exit_application(&app);
}

pub(crate) fn exit_application(app: &AppHandle) {
    app.exit(0);
}

#[tauri::command]
pub fn show_settings(app: AppHandle) -> Result<(), String> {
    show_settings_window(&app).map_err(|error_value| error_value.to_string())
}

#[tauri::command]
pub fn close_settings(app: AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("settings")
        .ok_or("Settings window unavailable")?;
    window.hide().map_err(|error_value| error_value.to_string())
}

#[tauri::command]
pub fn close(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    crate::picker::close(&app, &state)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum DeleteOutcome {
    Deleted,
    DeletedWithClipboardWarning,
}

#[tauri::command]
pub fn delete_item(
    app: AppHandle,
    state: State<'_, AppState>,
    hash: &str,
) -> Result<DeleteOutcome, String> {
    if hash.is_empty() {
        return Err("Clipboard item identity is empty".into());
    }

    let item_idx = state
        .clipboard
        .delete_by_hash(hash)
        .map_err(|error_value| error_value.to_string())?;

    if let Err(error_value) = app.emit_clipboard_changed() {
        error!(error:debug = error_value; "Failed to emit clipboard changed event");
    }

    if item_idx == 0 {
        let first_hash = match state.clipboard.first_hash() {
            Ok(first_hash) => first_hash,
            Err(error_value) => {
                error!(error:debug = error_value; "Failed to find the next clipboard item");
                return Ok(DeleteOutcome::DeletedWithClipboardWarning);
            }
        };
        if let Some(first_hash) = first_hash {
            let item = match state.clipboard.load_for_paste(&first_hash) {
                Ok(item) => item,
                Err(crate::storage::ClipboardError::ImageUnavailable {
                    fallback_text: Some(text),
                }) => PasteContent::Text(text),
                Err(error_value) => {
                    error!(error:debug = error_value; "Failed to load the next clipboard item");
                    return Ok(DeleteOutcome::DeletedWithClipboardWarning);
                }
            };
            let Some(system_clipboard) = state.system_clipboard.as_ref() else {
                error!(capability = "clipboard_write", failure_category = "adapter_unavailable"; "Clipboard write is unavailable");
                return Ok(DeleteOutcome::DeletedWithClipboardWarning);
            };

            if let Err(error_value) = system_clipboard.write_content(&item) {
                error!(error:debug = error_value; "Failed to write first item to clipboard");
                return Ok(DeleteOutcome::DeletedWithClipboardWarning);
            }
        }
    }
    Ok(DeleteOutcome::Deleted)
}

#[cfg(test)]
mod tests {
    use super::save_shortcut_transaction;
    use crate::content::PasteContent;
    use crate::picker::{paste_with, PasteOperations, PasteOutcome};
    use crate::settings::ShortcutSettings;

    struct FakePasteOperations {
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
        fn write_item(&self, _item: &PasteContent) -> Result<(), String> {
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
            paste_with(
                &operations,
                "text:known",
                &PasteContent::Text("copied text".into())
            ),
            Ok(PasteOutcome::CopiedForManualPaste)
        );
        assert_eq!(
            operations.events(),
            ["write", "reorder", "restore", "hide", "input"]
        );
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
    fn changing_only_picker_shortcuts_does_not_replace_the_global_binding() {
        let previous = ShortcutSettings::default();
        let mut next = previous.clone();
        next.move_selection_up = "KeyW".into();
        let mut replacements = 0;

        let saved = save_shortcut_transaction(
            true,
            &previous,
            &next,
            || Ok(()),
            |_, _| {
                replacements += 1;
                Ok(())
            },
            |_, _| Ok(()),
        );

        assert!(saved.is_ok());
        assert_eq!(replacements, 0);
    }

    #[test]
    fn persistence_failure_restores_the_previous_runtime_binding() {
        let previous = ShortcutSettings::default();
        let next = ShortcutSettings {
            open_klipo: "SUPER+ALT+KeyK".into(),
            ..ShortcutSettings::default()
        };
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
        let next = ShortcutSettings {
            open_klipo: "SUPER+ALT+KeyK".into(),
            ..ShortcutSettings::default()
        };
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
        let next = ShortcutSettings {
            open_klipo: "SUPER+ALT+KeyK".into(),
            ..ShortcutSettings::default()
        };
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
    }

    #[test]
    fn clipboard_write_failure_does_not_change_history_or_picker() {
        let mut operations = FakePasteOperations::successful();
        operations.write_result = FakePasteOperations::failed("write");

        assert_eq!(
            paste_with(
                &operations,
                "text:known",
                &PasteContent::Text("copied text".into())
            ),
            Err("write".into())
        );
        assert_eq!(operations.events(), ["write"]);
    }

    #[test]
    fn successful_paste_writes_reorders_restores_hides_then_inputs() {
        let operations = FakePasteOperations::successful();

        assert_eq!(
            paste_with(
                &operations,
                "text:known",
                &PasteContent::Text("copied text".into())
            ),
            Ok(PasteOutcome::Pasted)
        );
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
            paste_with(
                &operations,
                "text:known",
                &PasteContent::Text("copied text".into())
            ),
            Ok(PasteOutcome::CopiedForManualPaste)
        );
        assert_eq!(operations.events(), ["write", "reorder"]);
    }

    #[test]
    fn unavailable_target_keeps_picker_open_for_manual_paste() {
        let mut operations = FakePasteOperations::successful();
        operations.restore_result = FakePasteOperations::failed("target unavailable");

        assert_eq!(
            paste_with(
                &operations,
                "text:known",
                &PasteContent::Text("copied text".into())
            ),
            Ok(PasteOutcome::CopiedForManualPaste)
        );
        assert_eq!(operations.events(), ["write", "reorder", "restore"]);
    }

    #[test]
    fn hide_failure_does_not_attempt_input_or_report_success() {
        let mut operations = FakePasteOperations::successful();
        operations.hide_result = FakePasteOperations::failed("hide");

        assert_eq!(
            paste_with(
                &operations,
                "text:known",
                &PasteContent::Text("copied text".into())
            ),
            Ok(PasteOutcome::CopiedForManualPaste)
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
