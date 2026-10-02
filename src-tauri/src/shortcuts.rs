use log::error;
use tauri::{Manager, Runtime};
use tauri_plugin_global_shortcut::{
    GlobalShortcut, GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState,
};

use crate::desktop::DesktopSession;
use crate::settings::ShortcutSettings;
use crate::state::AppState;

pub fn supports_global_shortcuts(session: DesktopSession) -> bool {
    matches!(session, DesktopSession::Macos | DesktopSession::X11)
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

fn global_shortcut_binding(settings: &ShortcutSettings) -> Result<Shortcut, String> {
    settings
        .open_klipo
        .parse::<Shortcut>()
        .map_err(|_| "Open Klipo: unsupported shortcut".to_owned())
}

pub fn binding_is_registered(
    app: &tauri::AppHandle,
    settings: &ShortcutSettings,
) -> Result<bool, String> {
    let binding = global_shortcut_binding(settings)?;
    #[cfg(desktop)]
    {
        Ok(app.global_shortcut().is_registered(binding))
    }
    #[cfg(not(desktop))]
    {
        let _ = (app, binding);
        Ok(false)
    }
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
    if saved.validate().is_ok() {
        if let Ok(binding) = global_shortcut_binding(&saved) {
            if register_binding(registry, binding).is_ok() {
                return Ok(saved);
            }
        }
    }
    let defaults = ShortcutSettings::default();
    let binding = global_shortcut_binding(&defaults)?;
    register_binding(registry, binding)?;
    Ok(defaults)
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

    if supports_global_shortcuts(app.state::<AppState>().session) {
        let previous_binding = global_shortcut_binding(previous)?;
        let next_binding = global_shortcut_binding(next)?;

        #[cfg(desktop)]
        {
            let mut registry = TauriShortcutRegistry {
                shortcuts: app.global_shortcut(),
            };
            replace_binding(&mut registry, previous_binding, next_binding)?;
        }
    }

    Ok(())
}

pub fn cleanup_global_shortcuts(app: &tauri::AppHandle) -> Result<(), String> {
    if supports_global_shortcuts(app.state::<AppState>().session) {
        #[cfg(desktop)]
        {
            let shortcut_backend = app.try_state::<GlobalShortcut<tauri::Wry>>();

            if let Some(shortcut_backend) = shortcut_backend {
                let mut registry = TauriShortcutRegistry {
                    shortcuts: shortcut_backend.inner(),
                };
                cleanup_registry(&mut registry)?;
            }
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

fn register_binding(registry: &mut impl ShortcutRegistry, binding: Shortcut) -> Result<(), String> {
    if registry.is_registered(binding) {
        Ok(())
    } else {
        registry.register(binding).map_err(|error_value| {
            format!(
                "Open Klipo: {} could not register this shortcut ({error_value})",
                shortcut_backend_name()
            )
        })
    }
}

fn replace_binding(
    registry: &mut impl ShortcutRegistry,
    previous: Shortcut,
    next: Shortcut,
) -> Result<(), String> {
    if previous == next {
        return Ok(());
    }
    let previous_was_registered = registry.is_registered(previous);
    if previous_was_registered {
        registry.unregister(previous).map_err(|error_value| {
            format!(
                "{} could not release Open Klipo while replacing it ({error_value})",
                shortcut_backend_name()
            )
        })?;
    }
    if let Err(error_value) = register_binding(registry, next) {
        if previous_was_registered {
            match register_binding(registry, previous) {
                Ok(()) => Err(format!("{error_value}; restored the previous global shortcut binding")),
                Err(rollback_error) => Err(format!("{error_value}; failed to restore the previous global shortcut binding ({rollback_error})")),
            }
        } else {
            Err(error_value)
        }
    } else {
        Ok(())
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
    let Ok(binding) = global_shortcut_binding(&settings) else {
        error!("Failed to load shortcut binding");
        return;
    };
    if event.state() == ShortcutState::Pressed && &binding == shortcut {
        if let Err(error_value) = crate::picker::open(app) {
            error!(error:% = error_value; "Failed to open picker");
        }
    }
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

    fn binding(shortcut: &str) -> Shortcut {
        shortcut.parse().expect("test shortcut parses")
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn only_x11_sessions_are_eligible_for_the_linux_shortcut_backend() {
        assert!(supports_global_shortcuts(DesktopSession::X11));
        assert!(!supports_global_shortcuts(DesktopSession::Wayland));
        assert!(!supports_global_shortcuts(DesktopSession::Unknown));
    }

    #[test]
    fn macos_has_a_global_shortcut_backend() {
        assert!(supports_global_shortcuts(DesktopSession::Macos));
    }

    #[test]
    fn binding_selection_uses_only_the_global_open_klipo_shortcut() {
        let mut settings = ShortcutSettings {
            open_klipo: "SUPER+ALT+KeyK".into(),
            ..ShortcutSettings::default()
        };
        settings.move_selection_up = "KeyW".into();

        let bindings = global_shortcut_binding(&settings).expect("settings select a binding");

        assert_eq!(bindings, binding("SUPER+ALT+KeyK"));
    }

    #[test]
    fn failed_replacement_restores_the_previous_working_binding() {
        let previous = binding("SUPER+SHIFT+KeyV");
        let next = binding("SUPER+ALT+KeyK");
        let mut registry = FakeShortcutRegistry::with_registered(previous);
        registry.register_failures.insert(next);

        let error_value = replace_binding(&mut registry, previous, next)
            .expect_err("replacement fails when X11 registration is refused");

        assert!(error_value.contains("restored the previous"));
        assert!(registry.is_registered(previous));
        assert!(!registry.is_registered(next));
    }

    #[test]
    fn failed_old_binding_release_preserves_the_previous_working_binding() {
        let previous = binding("SUPER+SHIFT+KeyV");
        let next = binding("SUPER+ALT+KeyK");
        let mut registry = FakeShortcutRegistry::with_registered(previous);
        registry.unregister_failures.insert(previous);

        let error_value = replace_binding(&mut registry, previous, next)
            .expect_err("replacement stops when X11 cannot release the active binding");

        assert!(error_value.contains("could not release Open Klipo"));
        assert!(registry.is_registered(previous));
        assert!(!registry.is_registered(next));
        assert_eq!(
            registry.history,
            vec![RegistryOperation::Unregister(previous)]
        );
    }

    #[test]
    fn failed_previous_binding_restoration_reports_the_recovery_error() {
        let previous = binding("SUPER+SHIFT+KeyV");
        let next = binding("SUPER+ALT+KeyK");
        let mut registry = FakeShortcutRegistry::with_registered(previous);
        registry.register_failures.extend([previous, next]);

        let error_value = replace_binding(&mut registry, previous, next)
            .expect_err("new binding and restoration are both rejected");

        assert!(error_value.contains("failed to restore"));
        assert!(error_value.contains("backend refused registration"));
        assert!(!registry.is_registered(previous));
    }

    #[test]
    fn startup_selects_and_registers_the_saved_global_binding() {
        let saved = ShortcutSettings {
            open_klipo: "SUPER+ALT+KeyK".into(),
            ..ShortcutSettings::default()
        };
        let expected = binding("SUPER+ALT+KeyK");
        let mut registry = FakeShortcutRegistry::default();

        let active = register_saved_or_default_with(&mut registry, saved.clone())
            .expect("saved shortcut registration succeeds");

        assert_eq!(active, saved);
        assert!(registry.is_registered(expected));
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
        assert!(registry.is_registered(expected));
    }

    #[test]
    fn startup_reports_when_saved_and_default_registration_are_unavailable() {
        let defaults = ShortcutSettings::default();
        let default_binding = binding("SUPER+SHIFT+KeyV");
        let mut registry = FakeShortcutRegistry::default();
        registry.register_failures.insert(default_binding);

        let error_value = register_saved_or_default_with(&mut registry, defaults)
            .expect_err("startup reports an unavailable shortcut backend");

        assert!(error_value.contains("backend refused registration"));
    }

    #[test]
    fn repeated_save_and_cleanup_do_not_leave_registered_grabs() {
        let shortcut = binding("SUPER+SHIFT+KeyV");
        let mut registry = FakeShortcutRegistry::default();

        register_binding(&mut registry, shortcut).expect("first registration succeeds");
        replace_binding(&mut registry, shortcut, shortcut)
            .expect("same shortcut save is idempotent");
        cleanup_registry(&mut registry).expect("first cleanup succeeds");
        cleanup_registry(&mut registry).expect("second cleanup is idempotent");

        assert!(!registry.is_registered(shortcut));
    }

    #[test]
    fn cleanup_releases_divergent_plugin_owned_bindings() {
        let active = binding("SUPER+SHIFT+KeyV");
        let stale = binding("SUPER+ALT+KeyK");
        let mut registry = FakeShortcutRegistry::with_registered(stale);

        cleanup_registry(&mut registry).expect("cleanup releases all plugin-owned bindings");

        assert!(!registry.is_registered(active));
        assert!(!registry.is_registered(stale));
        assert_eq!(registry.history, vec![RegistryOperation::UnregisterAll]);
    }

    #[test]
    fn cleanup_reports_a_backend_failure_without_hiding_the_operation() {
        let shortcut = binding("SUPER+SHIFT+KeyV");
        let mut registry = FakeShortcutRegistry::with_registered(shortcut);
        registry.unregister_all_failure = true;

        let error_value = cleanup_registry(&mut registry)
            .expect_err("backend cleanup failures are returned to the shutdown caller");

        assert!(error_value.contains(shortcut_backend_name()));
        assert!(error_value.contains("backend refused release all"));
        assert!(registry.is_registered(shortcut));
        assert_eq!(registry.history, vec![RegistryOperation::UnregisterAll]);
    }
}
