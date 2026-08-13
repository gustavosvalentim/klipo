use serde::Serialize;

/// The desktop session detected from the runtime environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopSession {
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

    /// Capabilities supplied by a platform adapter. `AutomaticPaste` is
    /// derived from clipboard write, target restoration, and input.
    pub const PROBED: [Self; 8] = [
        Self::ClipboardRead,
        Self::ClipboardWrite,
        Self::Watcher,
        Self::Shortcut,
        Self::Pointer,
        Self::TargetRestoration,
        Self::Input,
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
    fn from_probe(result: Result<(), CapabilityUnavailableReason>) -> Self {
        match result {
            Ok(()) => Self::Available,
            Err(reason) => Self::Unavailable { reason },
        }
    }
}

/// Capability data safe to return across the application boundary.
///
/// Native window, display, and input identifiers deliberately do not appear in
/// this type. They are owned by the platform adapter that produced the probes.
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

/// Boundary for platform integrations. Implementations retain all native
/// handles; the domain receives only a success or stable failure reason.
pub trait DesktopAdapter {
    fn probe(&self, capability: DesktopCapability) -> Result<(), CapabilityUnavailableReason>;
}

/// Collect capability status without assuming an entire session is supported.
pub fn detect_capabilities(
    session: DesktopSession,
    adapter: &impl DesktopAdapter,
) -> DesktopCapabilities {
    DesktopCapabilities::from_statuses(session, |capability| {
        CapabilityStatus::from_probe(adapter.probe(capability))
    })
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
    detect_session_from(&ProcessEnvironment)
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

    struct FakeAdapter {
        results: HashMap<DesktopCapability, Result<(), CapabilityUnavailableReason>>,
    }

    impl FakeAdapter {
        fn available() -> Self {
            Self {
                results: DesktopCapability::PROBED
                    .into_iter()
                    .map(|capability| (capability, Ok(())))
                    .collect(),
            }
        }

        fn unavailable(
            mut self,
            capability: DesktopCapability,
            reason: CapabilityUnavailableReason,
        ) -> Self {
            self.results.insert(capability, Err(reason));
            self
        }

        fn unavailable_all(reason: CapabilityUnavailableReason) -> Self {
            Self {
                results: DesktopCapability::PROBED
                    .into_iter()
                    .map(|capability| (capability, Err(reason)))
                    .collect(),
            }
        }
    }

    impl DesktopAdapter for FakeAdapter {
        fn probe(&self, capability: DesktopCapability) -> Result<(), CapabilityUnavailableReason> {
            self.results
                .get(&capability)
                .copied()
                .expect("every capability has a fake probe result")
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
    fn reports_a_fully_available_session() {
        let capabilities = detect_capabilities(DesktopSession::X11, &FakeAdapter::available());

        assert_eq!(capabilities.session, DesktopSession::X11);
        for capability in DesktopCapability::ALL {
            assert_eq!(capabilities.status(capability), CapabilityStatus::Available);
        }
    }

    #[test]
    fn reports_partial_availability_without_inferencing_from_session() {
        let adapter = FakeAdapter::available()
            .unavailable(
                DesktopCapability::Shortcut,
                CapabilityUnavailableReason::InitializationFailed,
            )
            .unavailable(
                DesktopCapability::Input,
                CapabilityUnavailableReason::UnsupportedSession,
            );

        let capabilities = detect_capabilities(DesktopSession::Wayland, &adapter);

        assert_eq!(capabilities.clipboard_read, CapabilityStatus::Available);
        assert_eq!(
            capabilities.shortcut,
            CapabilityStatus::Unavailable {
                reason: CapabilityUnavailableReason::InitializationFailed,
            }
        );
        assert_eq!(
            capabilities.automatic_paste,
            CapabilityStatus::Unavailable {
                reason: CapabilityUnavailableReason::UnsupportedSession,
            }
        );
        assert_eq!(capabilities.tray, CapabilityStatus::Available);
    }

    #[test]
    fn reports_an_unknown_session_with_actionable_capability_reasons() {
        let adapter = FakeAdapter::unavailable_all(CapabilityUnavailableReason::UnknownSession);

        let capabilities = detect_capabilities(DesktopSession::Unknown, &adapter);

        assert_eq!(capabilities.session, DesktopSession::Unknown);
        assert_eq!(
            capabilities.clipboard_read,
            CapabilityStatus::Unavailable {
                reason: CapabilityUnavailableReason::UnknownSession,
            }
        );
        for capability in DesktopCapability::ALL {
            assert_eq!(
                capabilities.status(capability),
                CapabilityStatus::Unavailable {
                    reason: CapabilityUnavailableReason::UnknownSession,
                }
            );
        }
    }

    #[test]
    fn serializes_stable_capability_names_and_unavailable_reasons() {
        let adapter = FakeAdapter::available().unavailable(
            DesktopCapability::Watcher,
            CapabilityUnavailableReason::AdapterUnavailable,
        );
        let capabilities = detect_capabilities(DesktopSession::X11, &adapter);
        let value = serde_json::to_value(capabilities).expect("capabilities serialize");

        assert_eq!(value["clipboardRead"]["status"], json!("available"));
        assert_eq!(
            value["watcher"],
            json!({
                "status": "unavailable",
                "reason": "adapter_unavailable",
            })
        );
        assert_eq!(
            serde_json::to_value(CapabilityUnavailableReason::UnsupportedSession)
                .expect("reason serializes"),
            json!("unsupported_session")
        );
        assert_eq!(
            serde_json::to_value(CapabilityUnavailableReason::UnknownSession)
                .expect("reason serializes"),
            json!("unknown_session")
        );
        assert_eq!(
            serde_json::to_value(CapabilityUnavailableReason::InitializationFailed)
                .expect("reason serializes"),
            json!("initialization_failed")
        );
    }
}
