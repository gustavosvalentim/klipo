use enigo::Mouse;
use tauri::{LogicalPosition, LogicalSize, Manager, Position, WebviewWindow};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};

use crate::settings::ShortcutSettings;
use crate::state::AppState;
use crate::window::{capture_focused_window, get_main_window};

pub enum ShortcutError {
    InputError,
    PoisonError,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GlobalShortcutAction {
    OpenKlipo,
}

#[derive(Clone, Copy)]
struct GlobalShortcutBinding {
    action: GlobalShortcutAction,
    shortcut: Shortcut,
}

fn global_shortcut_bindings(
    settings: &ShortcutSettings,
) -> Result<Vec<GlobalShortcutBinding>, String> {
    let open_klipo = settings
        .open_klipo
        .parse::<Shortcut>()
        .map_err(|_| "Open Klipo: unsupported shortcut")?;

    Ok(vec![GlobalShortcutBinding {
        action: GlobalShortcutAction::OpenKlipo,
        shortcut: open_klipo,
    }])
}

fn show_on_cursor_handler(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();
    if let Err(e) = capture_focused_window(&state) {
        println!("Failed to get and store window state: {e}");
    }

    let Some(window) = get_main_window(app) else {
        println!("Failed to get main window");
        return;
    };

    let (mouse_x, mouse_y) = get_cursor_position(app).unwrap_or((0, 0));

    // TODO: handle multi monitor setups
    // Enigo uses logical coordinates. For reference, see:
    // https://v2.tauri.app/reference/javascript/api/namespacedpi/#logicalsize
    // https://v2.tauri.app/reference/javascript/api/namespacedpi/#physicalsize
    let window_size = get_window_logical_size(&window);
    let monitor_size = get_screen_logical_size(&window);

    let x = f64::from(mouse_x).clamp(0.0, monitor_size.width - window_size.width);
    let y = f64::from(mouse_y).clamp(0.0, monitor_size.height - window_size.height);
    let window_position = LogicalPosition { x, y };

    if let Err(e) = window.set_position(Position::Logical(window_position)) {
        println!("Failed to position window: {:?}", e);
        return;
    }

    let window = window.clone();
    // this is a hack to make the window appear on the correct
    // position without flickering.
    // Because tauri window methods are async, show() may run before
    // set_position() finishes, causing the window to briefly appear
    // on the old position before moving to the new one.
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;

        if let Err(e) = window.show() {
            println!("Failed to show window: {e}")
        }

        if let Err(e) = window.set_focus() {
            println!("Failed to focus window: {e}")
        }
    });
}

pub fn register_shortcuts_plugin(app: &tauri::AppHandle) -> Result<(), tauri::Error> {
    #[cfg(desktop)]
    {
        let global_shortcut_handler = tauri_plugin_global_shortcut::Builder::new()
            .with_handler(global_shortcut_handler)
            .build();

        app.plugin(global_shortcut_handler)?;
    }

    Ok(())
}

pub fn load_and_register_shortcuts(app: &tauri::AppHandle) -> Result<(), tauri::Error> {
    let path = settings_path(app)?;
    let saved = crate::settings::load(&path);
    let active_settings = register_saved_or_default(app, saved);
    let state = app.state::<AppState>();
    let mut shortcuts = state
        .shortcuts
        .lock()
        .map_err(|_| tauri::Error::WindowNotFound)?;
    *shortcuts = active_settings;
    Ok(())
}

fn register_saved_or_default(app: &tauri::AppHandle, saved: ShortcutSettings) -> ShortcutSettings {
    if saved.validate().is_ok() && register_global_shortcuts(app, &saved).is_ok() {
        return saved;
    }

    let defaults = ShortcutSettings::default();
    if let Err(error) = register_global_shortcuts(app, &defaults) {
        println!("Failed to register default shortcut: {error}");
    }
    defaults
}

fn register_global_shortcuts(
    app: &tauri::AppHandle,
    settings: &ShortcutSettings,
) -> Result<(), String> {
    settings.validate()?;
    let bindings = global_shortcut_bindings(settings)?;

    #[cfg(desktop)]
    {
        let shortcuts = app.global_shortcut();
        let mut registered = Vec::new();
        for binding in bindings {
            if shortcuts.is_registered(binding.shortcut) {
                continue;
            }
            if let Err(error) = shortcuts.register(binding.shortcut) {
                for shortcut in registered {
                    let _ = shortcuts.unregister(shortcut);
                }
                return Err(format!(
                    "{}: macOS could not register this shortcut ({error})",
                    next_binding_name(binding.action)
                ));
            }
            registered.push(binding.shortcut);
        }
    }
    Ok(())
}

pub fn replace_global_shortcuts(
    app: &tauri::AppHandle,
    previous: &ShortcutSettings,
    next: &ShortcutSettings,
) -> Result<(), String> {
    next.validate()?;
    let previous_bindings = global_shortcut_bindings(previous)?;
    let next_bindings = global_shortcut_bindings(next)?;

    #[cfg(desktop)]
    {
        replace_registered_shortcuts(app, &previous_bindings, &next_bindings)?;
    }
    Ok(())
}

#[cfg(desktop)]
fn replace_registered_shortcuts(
    app: &tauri::AppHandle,
    previous: &[GlobalShortcutBinding],
    next: &[GlobalShortcutBinding],
) -> Result<(), String> {
    let shortcuts = app.global_shortcut();
    let mut changes = Vec::new();
    for next_binding in next {
        match previous
            .iter()
            .find(|binding| binding.action == next_binding.action)
        {
            Some(previous_binding) if previous_binding.shortcut == next_binding.shortcut => {}
            Some(previous_binding) => changes.push((Some(*previous_binding), Some(*next_binding))),
            None => changes.push((None, Some(*next_binding))),
        }
    }
    for previous_binding in previous {
        if !next
            .iter()
            .any(|binding| binding.action == previous_binding.action)
        {
            changes.push((Some(*previous_binding), None));
        }
    }
    let previous_registered = changes
        .iter()
        .filter_map(|(previous, _)| previous.as_ref())
        .filter(|binding| shortcuts.is_registered(binding.shortcut))
        .map(|binding| binding.shortcut)
        .collect::<Vec<_>>();

    for shortcut in &previous_registered {
        shortcuts
            .unregister(*shortcut)
            .map_err(|error| error.to_string())?;
    }

    let mut registered = Vec::new();
    for next in changes.iter().filter_map(|(_, next)| next.as_ref()) {
        if shortcuts.is_registered(next.shortcut) {
            continue;
        }
        if let Err(error) = shortcuts.register(next.shortcut) {
            for shortcut in registered {
                let _ = shortcuts.unregister(shortcut);
            }
            for shortcut in previous_registered {
                let _ = shortcuts.register(shortcut);
            }
            return Err(format!(
                "{}: macOS could not register this shortcut ({error})",
                next_binding_name(next.action)
            ));
        }
        registered.push(next.shortcut);
    }
    Ok(())
}

fn next_binding_name(action: GlobalShortcutAction) -> &'static str {
    match action {
        GlobalShortcutAction::OpenKlipo => "Open Klipo",
    }
}

pub fn settings_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, tauri::Error> {
    Ok(app.path().app_config_dir()?.join("shortcuts.json"))
}

fn global_shortcut_handler(app: &tauri::AppHandle, shortcut: &Shortcut, event: ShortcutEvent) {
    let state = app.state::<AppState>();
    let Ok(settings) = state.shortcuts.lock() else {
        return;
    };
    let Ok(bindings) = global_shortcut_bindings(&settings) else {
        return;
    };
    let action = bindings
        .iter()
        .find(|binding| &binding.shortcut == shortcut)
        .map(|binding| binding.action);

    if event.state() == ShortcutState::Pressed && action == Some(GlobalShortcutAction::OpenKlipo) {
        show_on_cursor_handler(app);
    }
}

fn get_window_logical_size(window: &WebviewWindow) -> LogicalSize<f64> {
    let Ok(window_size) = window.inner_size() else {
        return LogicalSize {
            width: 0.0,
            height: 0.0,
        };
    };

    window_size.to_logical(window.scale_factor().unwrap_or(1.0))
}

fn get_screen_logical_size(window: &WebviewWindow) -> LogicalSize<f64> {
    let Ok(Some(monitor)) = window.current_monitor() else {
        return LogicalSize {
            width: 0.0,
            height: 0.0,
        };
    };

    monitor
        .size()
        .to_logical(window.scale_factor().unwrap_or(1.0))
}

fn get_cursor_position(app: &tauri::AppHandle) -> Result<(i32, i32), ShortcutError> {
    let state = app.state::<AppState>();
    let guard = state
        .input
        .enigo
        .lock()
        .map_err(|_| ShortcutError::PoisonError)?;
    let enigo = guard.as_ref().ok_or(ShortcutError::InputError)?;

    enigo.location().map_err(|_| ShortcutError::InputError)
}
