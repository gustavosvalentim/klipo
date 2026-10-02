use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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

pub struct AppState {
    pub session: DesktopSession,
    pub clipboard: ClipboardStore,
    pub system_clipboard: Option<SystemClipboard>,
    pub input: InputState,
    pub(crate) focus_target: Mutex<FocusTarget>,
    pub shortcuts: Mutex<ShortcutSettings>,
    clipboard_watcher_shutdown: Mutex<Option<WatcherShutdown>>,
    capabilities: Mutex<DesktopCapabilities>,
    picker_generation: AtomicU64,
    paste_in_progress: Mutex<()>,
    paste_focus_loss_expected: AtomicBool,
}

impl AppState {
    pub fn new(
        database_path: impl AsRef<Path>,
        session: DesktopSession,
    ) -> Result<Self, ClipboardError> {
        let unavailable_reason = match session {
            DesktopSession::Unknown => CapabilityUnavailableReason::UnknownSession,
            DesktopSession::Macos | DesktopSession::X11 | DesktopSession::Wayland => {
                CapabilityUnavailableReason::AdapterUnavailable
            }
        };

        Ok(Self {
            session,
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
            picker_generation: AtomicU64::new(0),
            paste_in_progress: Mutex::new(()),
            paste_focus_loss_expected: AtomicBool::new(false),
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

    pub fn next_picker_session(&self) -> u64 {
        self.paste_focus_loss_expected
            .store(false, Ordering::SeqCst);
        self.picker_generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn picker_session(&self) -> u64 {
        self.picker_generation.load(Ordering::SeqCst)
    }

    pub fn picker_session_is_current(&self, generation: u64) -> bool {
        self.picker_session() == generation
    }

    pub fn dismiss_picker_session(&self) {
        self.picker_generation.fetch_add(1, Ordering::SeqCst);
        self.paste_focus_loss_expected
            .store(false, Ordering::SeqCst);
    }

    pub fn expect_paste_focus_loss(&self, expected: bool) {
        self.paste_focus_loss_expected
            .store(expected, Ordering::SeqCst);
    }

    pub fn paste_focus_loss_expected(&self) -> bool {
        self.paste_focus_loss_expected.load(Ordering::SeqCst)
    }

    pub fn begin_paste(&self) -> Result<std::sync::MutexGuard<'_, ()>, String> {
        self.paste_in_progress
            .try_lock()
            .map_err(|error_value| match error_value {
                std::sync::TryLockError::WouldBlock => "A paste is already in progress".to_owned(),
                std::sync::TryLockError::Poisoned(_) => "Paste state is unavailable".to_owned(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::tempdir;

    #[test]
    fn dismissed_sessions_invalidate_pending_picker_work_and_pastes_do_not_overlap() {
        let directory = tempdir().unwrap();
        let state = AppState::new(
            directory.path().join("history.sqlite3"),
            DesktopSession::Macos,
        )
        .unwrap();
        let first_session = state.next_picker_session();
        let paste = state.begin_paste().unwrap();
        assert!(state.begin_paste().is_err());
        drop(paste);

        state.dismiss_picker_session();
        assert!(!state.picker_session_is_current(first_session));
        assert!(state.begin_paste().is_ok());
    }

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
