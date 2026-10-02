use serde::Serialize;

/// The desktop session detected from the runtime environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopSession {
    Macos,
    X11,
    Wayland,
    Unknown,
}

/// A desktop integration whose availability is reported independently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopCapability {
    ClipboardRead,
    ClipboardWrite,
    Watcher,
    Shortcut,
    Pointer,
    TargetRestoration,
    Input,
    AutomaticPaste,
    Tray,
}

impl DesktopCapability {
    pub const ALL: [Self; 9] = [
        Self::ClipboardRead,
        Self::ClipboardWrite,
        Self::Watcher,
        Self::Shortcut,
        Self::Pointer,
        Self::TargetRestoration,
        Self::Input,
        Self::AutomaticPaste,
        Self::Tray,
    ];
}

/// Stable, machine-readable reasons for an unavailable desktop integration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityUnavailableReason {
    UnsupportedSession,
    UnknownSession,
    AdapterUnavailable,
    InitializationFailed,
}

/// The availability of one desktop integration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CapabilityStatus {
    Available,
    Unavailable { reason: CapabilityUnavailableReason },
}

impl CapabilityStatus {
    pub const fn available() -> Self {
        Self::Available
    }

    pub const fn unavailable(reason: CapabilityUnavailableReason) -> Self {
        Self::Unavailable { reason }
    }
}

/// Capability data safe to return across the application boundary.
///
/// Native window, display, and input identifiers deliberately do not appear in
/// this type. Native integration modules retain those handles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopCapabilities {
    pub session: DesktopSession,
    pub clipboard_read: CapabilityStatus,
    pub clipboard_write: CapabilityStatus,
    pub watcher: CapabilityStatus,
    pub shortcut: CapabilityStatus,
    pub pointer: CapabilityStatus,
    pub target_restoration: CapabilityStatus,
    pub input: CapabilityStatus,
    /// This is derived from clipboard write, target restoration, and input.
    pub automatic_paste: CapabilityStatus,
    pub tray: CapabilityStatus,
}

impl DesktopCapabilities {
    pub fn unavailable(session: DesktopSession, reason: CapabilityUnavailableReason) -> Self {
        Self::from_statuses(session, |_| CapabilityStatus::unavailable(reason))
    }

    fn from_statuses(
        session: DesktopSession,
        mut status_for: impl FnMut(DesktopCapability) -> CapabilityStatus,
    ) -> Self {
        let clipboard_read = status_for(DesktopCapability::ClipboardRead);
        let clipboard_write = status_for(DesktopCapability::ClipboardWrite);
        let watcher = status_for(DesktopCapability::Watcher);
        let shortcut = status_for(DesktopCapability::Shortcut);
        let pointer = status_for(DesktopCapability::Pointer);
        let target_restoration = status_for(DesktopCapability::TargetRestoration);
        let input = status_for(DesktopCapability::Input);
        let tray = status_for(DesktopCapability::Tray);

        Self {
            session,
            clipboard_read,
            clipboard_write,
            watcher,
            shortcut,
            pointer,
            target_restoration,
            input,
            automatic_paste: automatic_paste_status(clipboard_write, target_restoration, input),
            tray,
        }
    }

    pub fn status(&self, capability: DesktopCapability) -> CapabilityStatus {
        match capability {
            DesktopCapability::ClipboardRead => self.clipboard_read,
            DesktopCapability::ClipboardWrite => self.clipboard_write,
            DesktopCapability::Watcher => self.watcher,
            DesktopCapability::Shortcut => self.shortcut,
            DesktopCapability::Pointer => self.pointer,
            DesktopCapability::TargetRestoration => self.target_restoration,
            DesktopCapability::Input => self.input,
            DesktopCapability::AutomaticPaste => self.automatic_paste,
            DesktopCapability::Tray => self.tray,
        }
    }

    pub fn set_status(&mut self, capability: DesktopCapability, status: CapabilityStatus) {
        match capability {
            DesktopCapability::ClipboardRead => self.clipboard_read = status,
            DesktopCapability::ClipboardWrite => self.clipboard_write = status,
            DesktopCapability::Watcher => self.watcher = status,
            DesktopCapability::Shortcut => self.shortcut = status,
            DesktopCapability::Pointer => self.pointer = status,
            DesktopCapability::TargetRestoration => self.target_restoration = status,
            DesktopCapability::Input => self.input = status,
            DesktopCapability::AutomaticPaste => {}
            DesktopCapability::Tray => self.tray = status,
        }

        self.automatic_paste =
            automatic_paste_status(self.clipboard_write, self.target_restoration, self.input);
    }
}

fn automatic_paste_status(
    clipboard_write: CapabilityStatus,
    target_restoration: CapabilityStatus,
    input: CapabilityStatus,
) -> CapabilityStatus {
    let unavailable_reason = [clipboard_write, target_restoration, input]
        .into_iter()
        .find_map(|status| match status {
            CapabilityStatus::Available => None,
            CapabilityStatus::Unavailable { reason } => Some(reason),
        });

    match unavailable_reason {
        Some(reason) => CapabilityStatus::Unavailable { reason },
        None => CapabilityStatus::Available,
    }
}

trait Environment {
    fn variable(&self, name: &str) -> Option<String>;
}

struct ProcessEnvironment;

impl Environment for ProcessEnvironment {
    fn variable(&self, name: &str) -> Option<String> {
        std::env::var(name).ok().filter(|value| !value.is_empty())
    }
}

/// Detect the active display protocol from runtime session variables.
pub fn detect_session() -> DesktopSession {
    if cfg!(target_os = "macos") {
        DesktopSession::Macos
    } else {
        detect_session_from(&ProcessEnvironment)
    }
}

fn detect_session_from(environment: &impl Environment) -> DesktopSession {
    let xdg_session_type = environment.variable("XDG_SESSION_TYPE");
    let has_wayland_display = environment.variable("WAYLAND_DISPLAY").is_some();
    let has_x11_display = environment.variable("DISPLAY").is_some();

    match xdg_session_type.as_deref() {
        Some("x11") => DesktopSession::X11,
        Some("wayland") => DesktopSession::Wayland,
        _ => match (has_wayland_display, has_x11_display) {
            (true, false) => DesktopSession::Wayland,
            (false, true) => DesktopSession::X11,
            _ => DesktopSession::Unknown,
        },
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::json;

    use super::*;

    struct FakeEnvironment {
        variables: HashMap<&'static str, &'static str>,
    }

    impl FakeEnvironment {
        fn with(variables: impl IntoIterator<Item = (&'static str, &'static str)>) -> Self {
            Self {
                variables: variables.into_iter().collect(),
            }
        }
    }

    impl Environment for FakeEnvironment {
        fn variable(&self, name: &str) -> Option<String> {
            self.variables.get(name).map(|value| (*value).to_owned())
        }
    }

    #[test]
    fn detects_x11_from_explicit_runtime_session() {
        let environment = FakeEnvironment::with([("XDG_SESSION_TYPE", "x11")]);

        assert_eq!(detect_session_from(&environment), DesktopSession::X11);
    }

    #[test]
    fn detects_wayland_from_display_when_session_type_is_missing() {
        let environment = FakeEnvironment::with([("WAYLAND_DISPLAY", "wayland-0")]);

        assert_eq!(detect_session_from(&environment), DesktopSession::Wayland);
    }

    #[test]
    fn reports_unknown_for_missing_or_ambiguous_fallback_signals() {
        let missing = FakeEnvironment::with([]);
        let ambiguous =
            FakeEnvironment::with([("WAYLAND_DISPLAY", "wayland-0"), ("DISPLAY", ":0")]);

        assert_eq!(detect_session_from(&missing), DesktopSession::Unknown);
        assert_eq!(detect_session_from(&ambiguous), DesktopSession::Unknown);
    }

    #[test]
    fn recognizes_wayland_when_xwayland_also_sets_display() {
        let environment = FakeEnvironment::with([
            ("XDG_SESSION_TYPE", "wayland"),
            ("WAYLAND_DISPLAY", "wayland-0"),
            ("DISPLAY", ":0"),
        ]);

        assert_eq!(detect_session_from(&environment), DesktopSession::Wayland);
    }

    #[test]
    fn capability_report_derives_automatic_paste_from_required_integrations() {
        let mut capabilities = DesktopCapabilities::unavailable(
            DesktopSession::X11,
            CapabilityUnavailableReason::AdapterUnavailable,
        );
        capabilities.set_status(
            DesktopCapability::ClipboardWrite,
            CapabilityStatus::Available,
        );
        capabilities.set_status(
            DesktopCapability::TargetRestoration,
            CapabilityStatus::Available,
        );
        assert_eq!(
            capabilities.automatic_paste,
            CapabilityStatus::unavailable(CapabilityUnavailableReason::AdapterUnavailable)
        );
        capabilities.set_status(DesktopCapability::Input, CapabilityStatus::Available);
        assert_eq!(capabilities.automatic_paste, CapabilityStatus::Available);
    }

    #[test]
    fn serializes_stable_capability_names_and_unavailable_reasons() {
        let mut capabilities = DesktopCapabilities::unavailable(
            DesktopSession::X11,
            CapabilityUnavailableReason::AdapterUnavailable,
        );
        capabilities.set_status(
            DesktopCapability::ClipboardRead,
            CapabilityStatus::Available,
        );
        let value = serde_json::to_value(capabilities).expect("capabilities serialize");
        assert_eq!(value["clipboardRead"]["status"], json!("available"));
        assert_eq!(value["watcher"]["reason"], json!("adapter_unavailable"));
        assert_eq!(
            serde_json::to_value(CapabilityUnavailableReason::UnknownSession).unwrap(),
            json!("unknown_session")
        );
    }
}
