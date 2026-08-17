use enigo::Mouse;
use log::{error, warn};
use tauri::{LogicalPosition, LogicalSize, Manager, Position, Runtime, WebviewWindow};
use tauri_plugin_global_shortcut::{
    GlobalShortcut, GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState,
};

#[cfg(target_os = "linux")]
use crate::desktop;
#[cfg(any(target_os = "linux", test))]
use crate::desktop::DesktopSession;
use crate::settings::ShortcutSettings;
use crate::state::AppState;
use crate::window::{capture_focused_window, get_main_window};

#[derive(Debug)]
pub enum ShortcutError {
    InputError,
    PoisonError,
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

pub fn initialize_global_shortcuts(app: &tauri::AppHandle) -> Result<(), tauri::Error> {
    if global_shortcuts_supported() {
        register_shortcuts_plugin(app)?;
        load_and_register_shortcuts(app)?;
    } else {
        let path = settings_path(app)?;
        let saved = crate::settings::load(&path);
        let active_settings = valid_or_default_settings(saved);
        let state = app.state::<AppState>();
        let mut shortcuts = state
            .shortcuts
            .lock()
            .map_err(|_| tauri::Error::WindowNotFound)?;
        *shortcuts = active_settings;
        #[cfg(target_os = "linux")]
        warn!(session:? = desktop::detect_session(); "Global shortcuts are disabled because the X11 backend is unavailable for this session");

        #[cfg(not(target_os = "linux"))]
        warn!("Global shortcuts are disabled because their backend is unavailable");
    }

    Ok(())
}

fn register_shortcuts_plugin(app: &tauri::AppHandle) -> Result<(), tauri::Error> {
    #[cfg(desktop)]
    {
        let global_shortcut_handler = tauri_plugin_global_shortcut::Builder::new()
            .with_handler(global_shortcut_handler)
            .build();

        app.plugin(global_shortcut_handler)?;
    }

    Ok(())
}

fn load_and_register_shortcuts(app: &tauri::AppHandle) -> Result<(), tauri::Error> {
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

fn valid_or_default_settings(settings: ShortcutSettings) -> ShortcutSettings {
    match settings.validate() {
        Ok(()) => settings,
        Err(_) => ShortcutSettings::default(),
    }
}

fn register_saved_or_default(app: &tauri::AppHandle, saved: ShortcutSettings) -> ShortcutSettings {
    #[cfg(desktop)]
    {
        let mut registry = TauriShortcutRegistry {
            shortcuts: app.global_shortcut(),
        };
        register_saved_or_default_with(&mut registry, saved)
    }

    #[cfg(not(desktop))]
    valid_or_default_settings(saved)
}

fn register_saved_or_default_with(
    registry: &mut impl ShortcutRegistry,
    saved: ShortcutSettings,
) -> ShortcutSettings {
    let saved_bindings = global_shortcut_bindings(&saved);
    if saved.validate().is_ok()
        && saved_bindings
            .as_ref()
            .is_ok_and(|bindings| register_bindings(registry, bindings).is_ok())
    {
        saved
    } else {
        let defaults = ShortcutSettings::default();
        match global_shortcut_bindings(&defaults) {
            Ok(bindings) => {
                if let Err(error_value) = register_bindings(registry, &bindings) {
                    error!(error:% = error_value; "Failed to register default global shortcut");
                }
            }
            Err(error_value) => {
                error!(error:% = error_value; "Failed to select default global shortcut");
            }
        }
        defaults
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
            let mut registry = TauriShortcutRegistry {
                shortcuts: app.global_shortcut(),
            };
            cleanup_registry(&mut registry)?;
        }
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

fn show_on_cursor_handler(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();
    if let Err(error_value) = capture_focused_window(&state) {
        error!(error:debug = error_value; "Failed to get and store window state");
    }

    let Some(window) = get_main_window(app) else {
        error!("Failed to get main window");
        return;
    };

    let (mouse_x, mouse_y) = get_cursor_position(app).unwrap_or_else(|error_value| {
        warn!(error:debug = error_value; "Failed to get cursor position; using origin");
        (0, 0)
    });

    let window_size = get_window_logical_size(&window);
    let monitor_size = get_screen_logical_size(&window);
    let x = f64::from(mouse_x).clamp(0.0, monitor_size.width - window_size.width);
    let y = f64::from(mouse_y).clamp(0.0, monitor_size.height - window_size.height);
    let window_position = LogicalPosition { x, y };

    if let Err(error_value) = window.set_position(Position::Logical(window_position)) {
        error!(error:debug = error_value; "Failed to position window");
        return;
    }

    let window = window.clone();
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

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

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

        let active = register_saved_or_default_with(&mut registry, saved.clone());

        assert_eq!(active, saved);
        assert!(registry.is_registered(expected.shortcut));
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
