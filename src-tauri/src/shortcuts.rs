use enigo::Mouse;
use log::{error, warn};
#[cfg(any(target_os = "linux", test))]
use tauri::PhysicalPosition;
#[cfg(not(target_os = "linux"))]
use tauri::{LogicalPosition, LogicalSize};
use tauri::{Manager, Position, Runtime, WebviewWindow};
use tauri_plugin_global_shortcut::{
    GlobalShortcut, GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState,
};

#[cfg(target_os = "linux")]
use crate::desktop;
use crate::desktop::DesktopSession;
use crate::settings::ShortcutSettings;
use crate::state::AppState;
use crate::window::{capture_focused_window, get_main_window};
#[cfg(target_os = "linux")]
use crate::window::{PICKER_HEIGHT, PICKER_WIDTH};

#[derive(Debug)]
pub enum ShortcutError {
    InputError,
    PoisonError,
}

pub fn supports_global_shortcuts(session: DesktopSession) -> bool {
    #[cfg(target_os = "linux")]
    {
        matches!(session, DesktopSession::X11)
    }

    #[cfg(target_os = "macos")]
    {
        let _ = session;
        true
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = session;
        false
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GlobalShortcutAction {
    OpenKlipo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GlobalShortcutBinding {
    action: GlobalShortcutAction,
    shortcut: Shortcut,
}

trait ShortcutRegistry {
    fn is_registered(&self, shortcut: Shortcut) -> bool;
    fn register(&mut self, shortcut: Shortcut) -> Result<(), String>;
    fn unregister(&mut self, shortcut: Shortcut) -> Result<(), String>;
    fn unregister_all(&mut self) -> Result<(), String>;
}

struct TauriShortcutRegistry<'a, R: Runtime> {
    shortcuts: &'a GlobalShortcut<R>,
}

impl<R: Runtime> ShortcutRegistry for TauriShortcutRegistry<'_, R> {
    fn is_registered(&self, shortcut: Shortcut) -> bool {
        self.shortcuts.is_registered(shortcut)
    }

    fn register(&mut self, shortcut: Shortcut) -> Result<(), String> {
        self.shortcuts
            .register(shortcut)
            .map_err(|error| error.to_string())
    }

    fn unregister(&mut self, shortcut: Shortcut) -> Result<(), String> {
        self.shortcuts
            .unregister(shortcut)
            .map_err(|error| error.to_string())
    }

    fn unregister_all(&mut self) -> Result<(), String> {
        self.shortcuts
            .unregister_all()
            .map_err(|error| error.to_string())
    }
}

/// The Tauri global-shortcut crate uses X11 on Linux. Do not initialize it in
/// Wayland or undetected sessions, even when XWayland also provides DISPLAY.
#[cfg(any(target_os = "linux", test))]
fn x11_shortcuts_supported(session: DesktopSession) -> bool {
    session == DesktopSession::X11
}

fn global_shortcuts_supported() -> bool {
    #[cfg(target_os = "linux")]
    {
        x11_shortcuts_supported(desktop::detect_session())
    }

    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}

fn shortcut_backend_name() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        "X11 global shortcut backend"
    }

    #[cfg(not(target_os = "linux"))]
    {
        "global shortcut backend"
    }
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
    if let Err(error_value) = capture_focused_window(&state) {
        error!(error:debug = error_value; "Failed to get and store window state");
    }

    let Some(window) = get_main_window(app) else {
        error!("Failed to get main window");
        return;
    };

    #[cfg(target_os = "linux")]
    let window_position = Position::Physical(get_linux_picker_position(app, &window));

    #[cfg(not(target_os = "linux"))]
    let window_position = Position::Logical(get_legacy_picker_position(app, &window));

    if let Err(error_value) = window.set_position(window_position) {
        error!(error:debug = error_value; "Failed to position window");
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

        if let Err(error_value) = window.show() {
            error!(error:debug = error_value; "Failed to show window");
        }

        if let Err(error_value) = window.set_focus() {
            error!(error:debug = error_value; "Failed to focus window");
        }
    });
}

#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PhysicalRectangle {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

#[cfg(any(target_os = "linux", test))]
impl PhysicalRectangle {
    fn contains(self, point: PhysicalPosition<i32>) -> bool {
        let point_x = i64::from(point.x);
        let point_y = i64::from(point.y);
        let right = i64::from(self.x) + i64::from(self.width);
        let bottom = i64::from(self.y) + i64::from(self.height);

        point_x >= i64::from(self.x)
            && point_x < right
            && point_y >= i64::from(self.y)
            && point_y < bottom
    }
}

#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, Debug, PartialEq)]
struct MonitorGeometry {
    bounds: PhysicalRectangle,
    work_area: PhysicalRectangle,
    is_primary: bool,
}

#[cfg(any(target_os = "linux", test))]
fn resolve_picker_position(
    pointer: Option<PhysicalPosition<i32>>,
    monitors: &[MonitorGeometry],
    picker_size: (u32, u32),
) -> Option<PhysicalPosition<i32>> {
    let monitor_at_pointer = pointer.and_then(|point| {
        monitors
            .iter()
            .find(|monitor| monitor.bounds.contains(point))
    });
    let selected_monitor = monitor_at_pointer
        .or_else(|| monitors.iter().find(|monitor| monitor.is_primary))
        .or_else(|| monitors.first())?;

    let fallback_position =
        PhysicalPosition::new(selected_monitor.work_area.x, selected_monitor.work_area.y);
    let pointer_position = if monitor_at_pointer.is_some() {
        pointer.unwrap_or(fallback_position)
    } else {
        fallback_position
    };

    Some(PhysicalPosition::new(
        clamp_picker_axis(
            pointer_position.x,
            selected_monitor.work_area.x,
            selected_monitor.work_area.width,
            picker_size.0,
        ),
        clamp_picker_axis(
            pointer_position.y,
            selected_monitor.work_area.y,
            selected_monitor.work_area.height,
            picker_size.1,
        ),
    ))
}

#[cfg(any(target_os = "linux", test))]
fn clamp_picker_axis(pointer: i32, work_start: i32, work_length: u32, picker_length: u32) -> i32 {
    let work_start = i64::from(work_start);
    let work_end = work_start + i64::from(work_length);
    let picker_end = work_end - i64::from(picker_length);
    let clamped_position = if picker_end >= work_start {
        i64::from(pointer).clamp(work_start, picker_end)
    } else {
        // A picker larger than the usable area cannot fit fully, so anchor it at
        // the work-area origin to keep as much of it visible as possible.
        work_start
    };

    clamped_position.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

#[cfg(target_os = "linux")]
fn get_linux_picker_position(
    app: &tauri::AppHandle,
    window: &WebviewWindow,
) -> PhysicalPosition<i32> {
    let picker_size = window
        .inner_size()
        .map(|size| (size.width, size.height))
        .unwrap_or_else(|error_value| {
            warn!(error:debug = error_value; "Failed to measure picker size; using configured size");
            (PICKER_WIDTH.round() as u32, PICKER_HEIGHT.round() as u32)
        });
    let monitors = match app.available_monitors() {
        Ok(monitors) => monitors,
        Err(error_value) => {
            warn!(error:debug = error_value; "Failed to list monitors; using origin");
            return PhysicalPosition::new(0, 0);
        }
    };
    let primary_monitor = match app.primary_monitor() {
        Ok(monitor) => monitor,
        Err(error_value) => {
            warn!(error:debug = error_value; "Failed to get primary monitor; using first monitor");
            None
        }
    };
    let monitor_geometries = monitors
        .iter()
        .map(|monitor| MonitorGeometry {
            bounds: PhysicalRectangle {
                x: monitor.position().x,
                y: monitor.position().y,
                width: monitor.size().width,
                height: monitor.size().height,
            },
            work_area: PhysicalRectangle {
                x: monitor.work_area().position.x,
                y: monitor.work_area().position.y,
                width: monitor.work_area().size.width,
                height: monitor.work_area().size.height,
            },
            is_primary: primary_monitor
                .as_ref()
                .is_some_and(|primary| monitors_match(monitor, primary)),
        })
        .collect::<Vec<_>>();
    let pointer = get_cursor_position(app)
        .map(|(x, y)| PhysicalPosition::new(x, y))
        .map_err(|error_value| {
            warn!(error:debug = error_value; "Failed to get cursor position; using primary monitor");
            error_value
        })
        .ok();

    resolve_picker_position(pointer, &monitor_geometries, picker_size).unwrap_or_else(|| {
        warn!("No monitor is available; using origin");
        PhysicalPosition::new(0, 0)
    })
}

#[cfg(target_os = "linux")]
fn monitors_match(left: &tauri::Monitor, right: &tauri::Monitor) -> bool {
    left.position() == right.position() && left.size() == right.size()
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

pub fn load_shortcut_settings(app: &tauri::AppHandle) -> Result<(), String> {
    let path = settings_path(app).map_err(|error| error.to_string())?;
    let saved = crate::settings::load(&path);
    let active_settings = valid_or_default_settings(saved);
    let state = app.state::<AppState>();
    let mut shortcuts = state
        .shortcuts
        .lock()
        .map_err(|_| "Shortcut settings are unavailable".to_owned())?;
    *shortcuts = active_settings;
    Ok(())
}

pub fn register_loaded_shortcuts(app: &tauri::AppHandle) -> Result<(), String> {
    let saved = app
        .state::<AppState>()
        .shortcuts
        .lock()
        .map_err(|_| "Shortcut settings are unavailable".to_owned())?
        .clone();
    let active_settings = register_saved_or_default(app, saved)?;
    let state = app.state::<AppState>();
    let mut shortcuts = state
        .shortcuts
        .lock()
        .map_err(|_| "Shortcut settings are unavailable".to_owned())?;
    *shortcuts = active_settings;
    Ok(())
}

fn register_saved_or_default(
    app: &tauri::AppHandle,
    saved: ShortcutSettings,
) -> Result<ShortcutSettings, String> {
    #[cfg(desktop)]
    {
        let mut registry = TauriShortcutRegistry {
            shortcuts: app.global_shortcut(),
        };
        register_saved_or_default_with(&mut registry, saved)
    }

    #[cfg(not(desktop))]
    Ok(valid_or_default_settings(saved))
}

fn register_saved_or_default_with(
    registry: &mut impl ShortcutRegistry,
    saved: ShortcutSettings,
) -> Result<ShortcutSettings, String> {
    let saved_bindings = global_shortcut_bindings(&saved);
    if saved.validate().is_ok()
        && saved_bindings
            .as_ref()
            .is_ok_and(|bindings| register_bindings(registry, bindings).is_ok())
    {
        Ok(saved)
    } else {
        let defaults = ShortcutSettings::default();
        let bindings = global_shortcut_bindings(&defaults)?;
        register_bindings(registry, &bindings)?;
        Ok(defaults)
    }
}

fn valid_or_default_settings(settings: ShortcutSettings) -> ShortcutSettings {
    match settings.validate() {
        Ok(()) => settings,
        Err(_) => ShortcutSettings::default(),
    }
}

pub fn replace_global_shortcuts(
    app: &tauri::AppHandle,
    previous: &ShortcutSettings,
    next: &ShortcutSettings,
) -> Result<(), String> {
    next.validate()?;

    if global_shortcuts_supported() {
        let previous_bindings = global_shortcut_bindings(previous)?;
        let next_bindings = global_shortcut_bindings(next)?;

        #[cfg(desktop)]
        {
            let mut registry = TauriShortcutRegistry {
                shortcuts: app.global_shortcut(),
            };
            replace_bindings(&mut registry, &previous_bindings, &next_bindings)?;
        }
    }

    Ok(())
}

pub fn cleanup_global_shortcuts(app: &tauri::AppHandle) -> Result<(), String> {
    if global_shortcuts_supported() {
        #[cfg(desktop)]
        {
            let shortcut_backend = app.try_state::<GlobalShortcut<tauri::Wry>>();

            if let Some(shortcut_backend) = shortcut_backend {
                let mut registry = TauriShortcutRegistry {
                    shortcuts: shortcut_backend.inner(),
                };
                cleanup_registered_shortcuts(Some(&mut registry))?;
            }
        }
    }

    Ok(())
}

fn cleanup_registered_shortcuts(
    registry: Option<&mut impl ShortcutRegistry>,
) -> Result<(), String> {
    if let Some(registry) = registry {
        cleanup_registry(registry)?;
    }

    Ok(())
}

fn cleanup_registry(registry: &mut impl ShortcutRegistry) -> Result<(), String> {
    registry.unregister_all().map_err(|error_value| {
        format!(
            "{} could not release shortcut resources ({error_value})",
            shortcut_backend_name()
        )
    })
}

fn register_bindings(
    registry: &mut impl ShortcutRegistry,
    bindings: &[GlobalShortcutBinding],
) -> Result<(), String> {
    let mut registered = Vec::new();

    for binding in bindings {
        if !registry.is_registered(binding.shortcut) {
            if let Err(error_value) = registry.register(binding.shortcut) {
                let rollback = unregister_bindings(registry, &registered);
                return Err(with_registration_cleanup_error(
                    format!(
                        "{}: {} could not register this shortcut ({error_value})",
                        next_binding_name(binding.action),
                        shortcut_backend_name()
                    ),
                    rollback,
                ));
            }
            registered.push(*binding);
        }
    }

    Ok(())
}

fn replace_bindings(
    registry: &mut impl ShortcutRegistry,
    previous: &[GlobalShortcutBinding],
    next: &[GlobalShortcutBinding],
) -> Result<(), String> {
    let changed_previous = previous
        .iter()
        .filter(|previous_binding| {
            !next.iter().any(|next_binding| {
                next_binding.action == previous_binding.action
                    && next_binding.shortcut == previous_binding.shortcut
            })
        })
        .copied()
        .collect::<Vec<_>>();
    let changed_next = next
        .iter()
        .filter(|next_binding| {
            !previous.iter().any(|previous_binding| {
                previous_binding.action == next_binding.action
                    && previous_binding.shortcut == next_binding.shortcut
            })
        })
        .copied()
        .collect::<Vec<_>>();

    let mut removed = Vec::new();
    for binding in changed_previous {
        if registry.is_registered(binding.shortcut) {
            if let Err(error_value) = registry.unregister(binding.shortcut) {
                let rollback = register_bindings(registry, &removed);
                return Err(with_recovery_error(
                    format!(
                        "{} could not release {} while replacing it ({error_value})",
                        shortcut_backend_name(),
                        next_binding_name(binding.action)
                    ),
                    rollback,
                ));
            }
            removed.push(binding);
        }
    }

    match register_bindings(registry, &changed_next) {
        Ok(()) => Ok(()),
        Err(error_value) => {
            let cleanup = unregister_bindings(registry, &changed_next);
            let restore = register_bindings(registry, &removed);
            let recovery = combine_recovery_results(cleanup, restore);
            Err(with_recovery_error(error_value, recovery))
        }
    }
}

fn unregister_bindings(
    registry: &mut impl ShortcutRegistry,
    bindings: &[GlobalShortcutBinding],
) -> Result<(), String> {
    let mut errors = Vec::new();

    for binding in bindings {
        if registry.is_registered(binding.shortcut) {
            if let Err(error_value) = registry.unregister(binding.shortcut) {
                errors.push(format!(
                    "{}: {error_value}",
                    next_binding_name(binding.action)
                ));
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} could not release shortcut resources ({})",
            shortcut_backend_name(),
            errors.join("; ")
        ))
    }
}

fn combine_recovery_results(
    first: Result<(), String>,
    second: Result<(), String>,
) -> Result<(), String> {
    match (first, second) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(first_error), Ok(())) => Err(first_error),
        (Ok(()), Err(second_error)) => Err(second_error),
        (Err(first_error), Err(second_error)) => Err(format!("{first_error}; {second_error}")),
    }
}

fn with_recovery_error(error_value: String, recovery: Result<(), String>) -> String {
    match recovery {
        Ok(()) => format!("{error_value}; restored the previous global shortcut binding"),
        Err(recovery_error) => format!(
			"{error_value}; failed to fully restore the previous global shortcut binding ({recovery_error})"
		),
    }
}

fn with_registration_cleanup_error(error_value: String, cleanup: Result<(), String>) -> String {
    match cleanup {
		Ok(()) => format!("{error_value}; removed partially registered global shortcut bindings"),
		Err(cleanup_error) => format!(
			"{error_value}; failed to release partially registered global shortcut bindings ({cleanup_error})"
		),
	}
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
        error!("Failed to lock shortcut settings");
        return;
    };
    let Ok(bindings) = global_shortcut_bindings(&settings) else {
        error!("Failed to load shortcut bindings");
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

#[cfg(not(target_os = "linux"))]
fn get_legacy_picker_position(
    app: &tauri::AppHandle,
    window: &WebviewWindow,
) -> LogicalPosition<f64> {
    let (mouse_x, mouse_y) = get_cursor_position(app).unwrap_or_else(|error_value| {
        warn!(error:debug = error_value; "Failed to get cursor position; using origin");
        (0, 0)
    });
    let window_size = get_window_logical_size(window);
    let monitor_size = get_screen_logical_size(window);

    LogicalPosition {
        x: f64::from(mouse_x).clamp(0.0, monitor_size.width - window_size.width),
        y: f64::from(mouse_y).clamp(0.0, monitor_size.height - window_size.height),
    }
}

#[cfg(not(target_os = "linux"))]
fn get_window_logical_size(window: &WebviewWindow) -> LogicalSize<f64> {
    let Ok(window_size) = window.inner_size() else {
        return LogicalSize {
            width: 0.0,
            height: 0.0,
        };
    };

    window_size.to_logical(window.scale_factor().unwrap_or(1.0))
}

#[cfg(not(target_os = "linux"))]
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

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    const PICKER_SIZE: (u32, u32) = (250, 350);

    fn monitor(
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        work_area: PhysicalRectangle,
        _scale_factor: f64,
        is_primary: bool,
    ) -> MonitorGeometry {
        MonitorGeometry {
            bounds: PhysicalRectangle {
                x,
                y,
                width,
                height,
            },
            work_area,
            is_primary,
        }
    }

    fn work_area(x: i32, y: i32, width: u32, height: u32) -> PhysicalRectangle {
        PhysicalRectangle {
            x,
            y,
            width,
            height,
        }
    }

    fn position_for(
        pointer: Option<(i32, i32)>,
        monitors: &[MonitorGeometry],
    ) -> PhysicalPosition<i32> {
        let pointer = pointer.map(|(x, y)| PhysicalPosition::new(x, y));

        resolve_picker_position(pointer, monitors, PICKER_SIZE)
            .expect("a monitor should resolve a position")
    }

    fn is_within_work_area(
        position: PhysicalPosition<i32>,
        picker_size: (u32, u32),
        work_area: PhysicalRectangle,
    ) -> bool {
        let picker_right = i64::from(position.x) + i64::from(picker_size.0);
        let picker_bottom = i64::from(position.y) + i64::from(picker_size.1);
        let work_right = i64::from(work_area.x) + i64::from(work_area.width);
        let work_bottom = i64::from(work_area.y) + i64::from(work_area.height);

        i64::from(position.x) >= i64::from(work_area.x)
            && picker_right <= work_right
            && i64::from(position.y) >= i64::from(work_area.y)
            && picker_bottom <= work_bottom
    }

    fn intersects_work_area(
        position: PhysicalPosition<i32>,
        picker_size: (u32, u32),
        work_area: PhysicalRectangle,
    ) -> bool {
        let picker_right = i64::from(position.x) + i64::from(picker_size.0);
        let picker_bottom = i64::from(position.y) + i64::from(picker_size.1);
        let work_right = i64::from(work_area.x) + i64::from(work_area.width);
        let work_bottom = i64::from(work_area.y) + i64::from(work_area.height);

        i64::from(position.x) < work_right
            && picker_right > i64::from(work_area.x)
            && i64::from(position.y) < work_bottom
            && picker_bottom > i64::from(work_area.y)
    }

    #[test]
    fn positions_picker_on_primary_monitor_and_clamps_at_its_edges() {
        let primary = monitor(0, 0, 1920, 1080, work_area(0, 0, 1920, 1040), 1.0, true);

        assert_eq!(
            position_for(Some((500, 400)), &[primary]),
            PhysicalPosition::new(500, 400)
        );
        assert_eq!(
            position_for(Some((1919, 1079)), &[primary]),
            PhysicalPosition::new(1670, 690)
        );
    }

    #[test]
    fn selects_secondary_monitors_with_positive_and_negative_origins() {
        let left = monitor(
            -1280,
            0,
            1280,
            1024,
            work_area(-1280, 0, 1280, 1024),
            1.0,
            false,
        );
        let primary = monitor(0, 0, 1920, 1080, work_area(0, 0, 1920, 1080), 1.0, true);
        let right = monitor(
            1920,
            0,
            1600,
            900,
            work_area(1920, 0, 1600, 900),
            1.0,
            false,
        );

        assert_eq!(
            position_for(Some((-1200, 200)), &[left, primary, right]),
            PhysicalPosition::new(-1200, 200)
        );
        assert_eq!(
            position_for(Some((3500, 850)), &[left, primary, right]),
            PhysicalPosition::new(3270, 550)
        );
    }

    #[test]
    fn preserves_negative_vertical_origins() {
        let above_primary = monitor(
            0,
            -900,
            1600,
            900,
            work_area(0, -900, 1600, 900),
            1.0,
            false,
        );
        let primary = monitor(0, 0, 1920, 1080, work_area(0, 0, 1920, 1080), 1.0, true);

        assert_eq!(
            position_for(Some((400, -10)), &[above_primary, primary]),
            PhysicalPosition::new(400, -350)
        );
    }

    #[test]
    fn clamps_to_work_area_instead_of_full_monitor_bounds() {
        let primary = monitor(0, 0, 1920, 1080, work_area(0, 32, 1920, 1008), 1.0, true);

        assert_eq!(
            position_for(Some((20, 10)), &[primary]),
            PhysicalPosition::new(20, 32)
        );
        assert_eq!(
            position_for(Some((1900, 1070)), &[primary]),
            PhysicalPosition::new(1670, 690)
        );
    }

    #[test]
    fn uses_measured_picker_size_on_mixed_scale_monitors() {
        let primary = monitor(0, 0, 1920, 1080, work_area(0, 0, 1920, 1080), 1.0, true);
        // X11 can report the primary output's scale for every monitor even when
        // the secondary output transforms its pixels differently.
        let scaled_secondary = monitor(
            1920,
            0,
            2560,
            1440,
            work_area(1920, 0, 2560, 1440),
            1.0,
            false,
        );

        // This is the physical size measured from the picker window itself.
        assert_eq!(
            resolve_picker_position(
                Some(PhysicalPosition::new(4400, 1300)),
                &[primary, scaled_secondary],
                (375, 525),
            )
            .expect("a monitor should resolve a position"),
            PhysicalPosition::new(4105, 915)
        );
    }

    #[test]
    fn falls_back_to_primary_or_first_monitor_when_pointer_is_unavailable() {
        let left = monitor(
            -1280,
            0,
            1280,
            1024,
            work_area(-1280, 0, 1280, 1024),
            1.0,
            false,
        );
        let negative_origin_primary = monitor(
            -1920,
            0,
            640,
            480,
            work_area(-1920, 20, 640, 460),
            1.0,
            true,
        );

        assert_eq!(
            position_for(None, &[left, negative_origin_primary]),
            PhysicalPosition::new(-1920, 20)
        );
        assert_eq!(position_for(None, &[left]), PhysicalPosition::new(-1280, 0));
    }

    #[test]
    fn falls_back_when_pointer_is_outside_every_monitor() {
        let primary = monitor(
            -1920,
            0,
            1920,
            1080,
            work_area(-1920, 0, 1920, 1080),
            1.0,
            true,
        );

        assert_eq!(
            position_for(Some((500, 500)), &[primary]),
            PhysicalPosition::new(-1920, 0)
        );
    }

    #[test]
    fn topology_matrix_keeps_a_fitting_picker_inside_each_work_area() {
        let monitors = [
            monitor(
                -1600,
                -900,
                1600,
                900,
                work_area(-1600, -900, 1600, 900),
                1.0,
                false,
            ),
            monitor(0, 0, 1920, 1080, work_area(0, 24, 1920, 1056), 1.0, true),
            monitor(
                1920,
                0,
                2560,
                1440,
                work_area(1920, 0, 2560, 1400),
                1.5,
                false,
            ),
        ];
        let pointer_positions = [
            (-1600, -900),
            (-1, -1),
            (0, 0),
            (1919, 1079),
            (1920, 0),
            (4479, 1439),
        ];

        for pointer in pointer_positions {
            let position = position_for(Some(pointer), &monitors);
            let selected_monitor = monitors
                .iter()
                .find(|monitor| {
                    monitor
                        .bounds
                        .contains(PhysicalPosition::new(pointer.0, pointer.1))
                })
                .expect("test pointer should be on a monitor");

            assert!(is_within_work_area(
                position,
                PICKER_SIZE,
                selected_monitor.work_area,
            ));
        }
    }

    #[derive(Default)]
    struct FakeShortcutRegistry {
        registered: HashSet<Shortcut>,
        register_failures: HashSet<Shortcut>,
        unregister_failures: HashSet<Shortcut>,
        unregister_all_failure: bool,
        history: Vec<RegistryOperation>,
    }

    #[derive(Debug, PartialEq, Eq)]
    enum RegistryOperation {
        Register(Shortcut),
        Unregister(Shortcut),
        UnregisterAll,
    }

    impl FakeShortcutRegistry {
        fn with_registered(shortcut: Shortcut) -> Self {
            Self {
                registered: HashSet::from([shortcut]),
                ..Self::default()
            }
        }
    }

    impl ShortcutRegistry for FakeShortcutRegistry {
        fn is_registered(&self, shortcut: Shortcut) -> bool {
            self.registered.contains(&shortcut)
        }

        fn register(&mut self, shortcut: Shortcut) -> Result<(), String> {
            self.history.push(RegistryOperation::Register(shortcut));
            if self.register_failures.contains(&shortcut) {
                Err("backend refused registration".into())
            } else {
                self.registered.insert(shortcut);
                Ok(())
            }
        }

        fn unregister(&mut self, shortcut: Shortcut) -> Result<(), String> {
            self.history.push(RegistryOperation::Unregister(shortcut));
            if self.unregister_failures.contains(&shortcut) {
                Err("backend refused release".into())
            } else {
                self.registered.remove(&shortcut);
                Ok(())
            }
        }

        fn unregister_all(&mut self) -> Result<(), String> {
            self.history.push(RegistryOperation::UnregisterAll);
            if self.unregister_all_failure {
                Err("backend refused release all".into())
            } else {
                self.registered.clear();
                Ok(())
            }
        }
    }

    fn binding(shortcut: &str) -> GlobalShortcutBinding {
        GlobalShortcutBinding {
            action: GlobalShortcutAction::OpenKlipo,
            shortcut: shortcut.parse().expect("test shortcut parses"),
        }
    }

    #[test]
    fn oversized_picker_stays_visible_instead_of_resolving_outside_the_work_area() {
        let primary = monitor(
            -100,
            -100,
            200,
            200,
            work_area(-100, -100, 200, 200),
            1.0,
            true,
        );
        let position = position_for(Some((-50, -50)), &[primary]);

        assert_eq!(position, PhysicalPosition::new(-100, -100));
        assert!(intersects_work_area(
            position,
            PICKER_SIZE,
            primary.work_area
        ));
    }

    #[test]
    fn only_x11_sessions_are_eligible_for_the_linux_shortcut_backend() {
        assert!(x11_shortcuts_supported(DesktopSession::X11));
        assert!(!x11_shortcuts_supported(DesktopSession::Wayland));
        assert!(!x11_shortcuts_supported(DesktopSession::Unknown));
    }

    #[test]
    fn binding_selection_uses_only_the_global_open_klipo_shortcut() {
        let mut settings = ShortcutSettings::default();
        settings.open_klipo = "SUPER+ALT+KeyK".into();
        settings.move_selection_up = "KeyW".into();

        let bindings = global_shortcut_bindings(&settings).expect("settings select a binding");

        assert_eq!(bindings, vec![binding("SUPER+ALT+KeyK")]);
    }

    #[test]
    fn failed_replacement_restores_the_previous_working_binding() {
        let previous = binding("SUPER+SHIFT+KeyV");
        let next = binding("SUPER+ALT+KeyK");
        let mut registry = FakeShortcutRegistry::with_registered(previous.shortcut);
        registry.register_failures.insert(next.shortcut);

        let error_value = replace_bindings(&mut registry, &[previous], &[next])
            .expect_err("replacement fails when X11 registration is refused");

        assert!(error_value.contains("restored the previous"));
        assert!(registry.is_registered(previous.shortcut));
        assert!(!registry.is_registered(next.shortcut));
    }

    #[test]
    fn failed_old_binding_release_preserves_the_previous_working_binding() {
        let previous = binding("SUPER+SHIFT+KeyV");
        let next = binding("SUPER+ALT+KeyK");
        let mut registry = FakeShortcutRegistry::with_registered(previous.shortcut);
        registry.unregister_failures.insert(previous.shortcut);

        let error_value = replace_bindings(&mut registry, &[previous], &[next])
            .expect_err("replacement stops when X11 cannot release the active binding");

        assert!(error_value.contains("could not release Open Klipo"));
        assert!(registry.is_registered(previous.shortcut));
        assert!(!registry.is_registered(next.shortcut));
        assert_eq!(
            registry.history,
            vec![RegistryOperation::Unregister(previous.shortcut)]
        );
    }

    #[test]
    fn partial_new_registration_is_cleaned_up_before_returning_an_error() {
        let first = binding("SUPER+ALT+KeyK");
        let second = binding("SUPER+ALT+KeyL");
        let mut registry = FakeShortcutRegistry::default();
        registry.register_failures.insert(second.shortcut);

        let error_value = register_bindings(&mut registry, &[first, second])
            .expect_err("second registration is rejected");

        assert!(error_value.contains("removed partially registered"));
        assert!(!registry.is_registered(first.shortcut));
        assert_eq!(
            registry.history,
            vec![
                RegistryOperation::Register(first.shortcut),
                RegistryOperation::Register(second.shortcut),
                RegistryOperation::Unregister(first.shortcut),
            ]
        );
    }

    #[test]
    fn failed_previous_binding_restoration_reports_the_recovery_error() {
        let previous = binding("SUPER+SHIFT+KeyV");
        let next = binding("SUPER+ALT+KeyK");
        let mut registry = FakeShortcutRegistry::with_registered(previous.shortcut);
        registry
            .register_failures
            .extend([previous.shortcut, next.shortcut]);

        let error_value = replace_bindings(&mut registry, &[previous], &[next])
            .expect_err("new binding and restoration are both rejected");

        assert!(error_value.contains("failed to fully restore"));
        assert!(error_value.contains("backend refused registration"));
        assert!(!registry.is_registered(previous.shortcut));
    }

    #[test]
    fn startup_selects_and_registers_the_saved_global_binding() {
        let mut saved = ShortcutSettings::default();
        saved.open_klipo = "SUPER+ALT+KeyK".into();
        let expected = binding("SUPER+ALT+KeyK");
        let mut registry = FakeShortcutRegistry::default();

        let active = register_saved_or_default_with(&mut registry, saved.clone())
            .expect("saved shortcut registration succeeds");

        assert_eq!(active, saved);
        assert!(registry.is_registered(expected.shortcut));
    }

    #[test]
    fn startup_uses_defaults_when_the_saved_shortcut_is_invalid() {
        let saved = ShortcutSettings {
            open_klipo: "Escape".into(),
            ..ShortcutSettings::default()
        };
        let expected = binding("SUPER+SHIFT+KeyV");
        let mut registry = FakeShortcutRegistry::default();

        let active = register_saved_or_default_with(&mut registry, saved)
            .expect("default shortcut registration succeeds");

        assert_eq!(active, ShortcutSettings::default());
        assert!(registry.is_registered(expected.shortcut));
    }

    #[test]
    fn startup_reports_when_saved_and_default_registration_are_unavailable() {
        let defaults = ShortcutSettings::default();
        let default_binding = binding("SUPER+SHIFT+KeyV");
        let mut registry = FakeShortcutRegistry::default();
        registry.register_failures.insert(default_binding.shortcut);

        let error_value = register_saved_or_default_with(&mut registry, defaults)
            .expect_err("startup reports an unavailable shortcut backend");

        assert!(error_value.contains("backend refused registration"));
    }

    #[test]
    fn repeated_save_and_cleanup_do_not_leave_registered_grabs() {
        let shortcut = binding("SUPER+SHIFT+KeyV");
        let mut registry = FakeShortcutRegistry::default();

        register_bindings(&mut registry, &[shortcut]).expect("first registration succeeds");
        replace_bindings(&mut registry, &[shortcut], &[shortcut])
            .expect("same shortcut save is idempotent");
        cleanup_registry(&mut registry).expect("first cleanup succeeds");
        cleanup_registry(&mut registry).expect("second cleanup is idempotent");

        assert!(!registry.is_registered(shortcut.shortcut));
    }

    #[test]
    fn cleanup_releases_divergent_plugin_owned_bindings() {
        let active = binding("SUPER+SHIFT+KeyV");
        let stale = binding("SUPER+ALT+KeyK");
        let mut registry = FakeShortcutRegistry::with_registered(stale.shortcut);

        cleanup_registry(&mut registry).expect("cleanup releases all plugin-owned bindings");

        assert!(!registry.is_registered(active.shortcut));
        assert!(!registry.is_registered(stale.shortcut));
        assert_eq!(registry.history, vec![RegistryOperation::UnregisterAll]);
    }

    #[test]
    fn cleanup_without_an_initialized_backend_is_a_noop() {
        let registry: Option<&mut FakeShortcutRegistry> = None;

        cleanup_registered_shortcuts(registry)
            .expect("cleanup skips an unavailable shortcut backend");
    }

    #[test]
    fn cleanup_reports_a_backend_failure_without_hiding_the_operation() {
        let shortcut = binding("SUPER+SHIFT+KeyV");
        let mut registry = FakeShortcutRegistry::with_registered(shortcut.shortcut);
        registry.unregister_all_failure = true;

        let error_value = cleanup_registry(&mut registry)
            .expect_err("backend cleanup failures are returned to the shutdown caller");

        assert!(error_value.contains(shortcut_backend_name()));
        assert!(error_value.contains("backend refused release all"));
        assert!(registry.is_registered(shortcut.shortcut));
        assert_eq!(registry.history, vec![RegistryOperation::UnregisterAll]);
    }
}
