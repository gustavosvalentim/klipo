use tauri::window::{Effect, EffectState, EffectsBuilder};
use tauri::{WebviewUrl, WebviewWindow, WebviewWindowBuilder, Window, WindowEvent};

pub struct Settings {
    pub width: f64,
    pub height: f64,
    pub transparent: bool,
    pub decorations: bool,
    pub radius: f64,
}

#[derive(Debug)]
pub enum WindowError {
    TauriError(tauri::Error),
}

impl std::fmt::Display for WindowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WindowError::TauriError(e) => write!(f, "Tauri error: {e}"),
        }
    }
}

pub fn create_clipbox_window(
    app: &tauri::AppHandle,
    settings: Settings,
) -> Result<WebviewWindow, WindowError> {
    let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
        .inner_size(settings.width, settings.height)
        .decorations(settings.decorations)
        .transparent(settings.transparent)
        .always_on_top(true)
        .visible(false)
        .visible_on_all_workspaces(true)
        .shadow(true)
        .effects(
            EffectsBuilder::new()
                .effect(Effect::Menu)
                .state(EffectState::Active)
                .radius(settings.radius)
                .build(),
        )
        .build();

    let window = match window {
        Ok(window) => window,
        Err(e) => return Err(WindowError::TauriError(e)),
    };

    Ok(window)
}

pub fn window_events_handler(window: &Window, event: &WindowEvent) {
    if let WindowEvent::Focused(focused) = event {
        if !focused {
            let _ = window.hide();
        }
    }
}
