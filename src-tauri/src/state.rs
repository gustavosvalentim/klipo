use std::path::Path;
use std::sync::Mutex;

use clipboard_rs::WatcherShutdown;

use crate::clipboard::SystemClipboard;
use crate::desktop::{
    CapabilityStatus, CapabilityUnavailableReason, DesktopCapabilities, DesktopCapability,
    DesktopSession,
};
use crate::input::InputState;
use crate::settings::ShortcutSettings;
use crate::storage::{ClipboardError, ClipboardStore};
use crate::window::FocusTarget;

#[derive(Debug)]
pub enum AppStateError {
    Clipboard(ClipboardError),
}

impl std::fmt::Display for AppStateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Clipboard(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for AppStateError {}

impl From<ClipboardError> for AppStateError {
    fn from(error: ClipboardError) -> Self {
        Self::Clipboard(error)
    }
}

pub struct AppState {
    pub clipboard: ClipboardStore,
    pub system_clipboard: Option<SystemClipboard>,
    pub input: InputState,
    pub(crate) focus_target: Mutex<FocusTarget>,
    pub shortcuts: Mutex<ShortcutSettings>,
    clipboard_watcher_shutdown: Mutex<Option<WatcherShutdown>>,
    capabilities: Mutex<DesktopCapabilities>,
}

impl AppState {
    pub fn new(
        database_path: impl AsRef<Path>,
        session: DesktopSession,
    ) -> Result<Self, AppStateError> {
        let unavailable_reason = match session {
            DesktopSession::Unknown => CapabilityUnavailableReason::UnknownSession,
            DesktopSession::X11 | DesktopSession::Wayland => {
                CapabilityUnavailableReason::AdapterUnavailable
            }
        };

        Ok(Self {
            clipboard: ClipboardStore::open(database_path)?,
            system_clipboard: None,
            input: InputState::new(),
            focus_target: Mutex::new(FocusTarget::empty()),
            shortcuts: Mutex::new(ShortcutSettings::default()),
            clipboard_watcher_shutdown: Mutex::new(None),
            capabilities: Mutex::new(DesktopCapabilities::unavailable(
                session,
                unavailable_reason,
            )),
        })
    }

    pub fn install_system_clipboard(&mut self, system_clipboard: SystemClipboard) {
        self.system_clipboard = Some(system_clipboard);
        self.set_capability(
            DesktopCapability::ClipboardRead,
            CapabilityStatus::available(),
        );
        self.set_capability(
            DesktopCapability::ClipboardWrite,
            CapabilityStatus::available(),
        );
    }

    pub fn install_clipboard_watcher(&self, watcher_shutdown: WatcherShutdown) {
        if let Ok(mut current_shutdown) = self.clipboard_watcher_shutdown.lock() {
            *current_shutdown = Some(watcher_shutdown);
        }
    }

    pub fn shutdown_clipboard_watcher(&self) {
        if let Ok(mut current_shutdown) = self.clipboard_watcher_shutdown.lock() {
            drop(current_shutdown.take());
        }

        if let Some(system_clipboard) = self.system_clipboard.as_ref() {
            system_clipboard.shutdown();
        }
    }

    pub fn set_capability(&self, capability: DesktopCapability, status: CapabilityStatus) {
        if let Ok(mut capabilities) = self.capabilities.lock() {
            capabilities.set_status(capability, status);
        }
    }

    pub fn replace_capabilities(&self, capabilities: DesktopCapabilities) {
        if let Ok(mut current) = self.capabilities.lock() {
            *current = capabilities;
        }
    }

    pub fn capability_is_available(&self, capability: DesktopCapability) -> bool {
        self.capabilities()
            .map(|capabilities| capabilities.status(capability) == CapabilityStatus::available())
            .unwrap_or(false)
    }

    pub fn capabilities(&self) -> Result<DesktopCapabilities, String> {
        self.capabilities
            .lock()
            .map(|capabilities| capabilities.clone())
            .map_err(|_| "Desktop capability state is unavailable".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independently_updates_capabilities_without_disabling_the_session() {
        let mut capabilities = DesktopCapabilities::unavailable(
            DesktopSession::X11,
            CapabilityUnavailableReason::AdapterUnavailable,
        );

        capabilities.set_status(
            DesktopCapability::ClipboardRead,
            CapabilityStatus::available(),
        );
        capabilities.set_status(
            DesktopCapability::ClipboardWrite,
            CapabilityStatus::available(),
        );
        capabilities.set_status(DesktopCapability::Input, CapabilityStatus::available());
        capabilities.set_status(
            DesktopCapability::TargetRestoration,
            CapabilityStatus::unavailable(CapabilityUnavailableReason::UnsupportedSession),
        );
        capabilities.set_status(DesktopCapability::Tray, CapabilityStatus::available());

        assert_eq!(capabilities.clipboard_read, CapabilityStatus::available());
        assert_eq!(
            capabilities.shortcut,
            CapabilityStatus::unavailable(CapabilityUnavailableReason::AdapterUnavailable)
        );
        assert_eq!(
            capabilities.automatic_paste,
            CapabilityStatus::unavailable(CapabilityUnavailableReason::UnsupportedSession)
        );
        assert_eq!(capabilities.tray, CapabilityStatus::available());
    }
}
