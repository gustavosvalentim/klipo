use crate::desktop::{
    CapabilityStatus, CapabilityUnavailableReason, DesktopCapabilities, DesktopCapability,
    DesktopSession,
};

/// Records best-effort desktop initialization without allowing an optional
/// integration failure to stop the application shell.
pub struct StartupCoordinator {
    capabilities: DesktopCapabilities,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupStep {
    Clipboard,
    Input,
    TargetRestoration,
    Shortcut,
    Tray,
    Watcher,
    Window,
}

impl StartupStep {
    pub const ALL: [Self; 7] = [
        Self::Clipboard,
        Self::Input,
        Self::TargetRestoration,
        Self::Shortcut,
        Self::Tray,
        Self::Watcher,
        Self::Window,
    ];
}

impl StartupCoordinator {
    pub fn new(session: DesktopSession) -> Self {
        let unavailable_reason = match session {
            DesktopSession::Unknown => CapabilityUnavailableReason::UnknownSession,
            DesktopSession::X11 | DesktopSession::Wayland => {
                CapabilityUnavailableReason::AdapterUnavailable
            }
        };

        Self {
            capabilities: DesktopCapabilities::unavailable(session, unavailable_reason),
        }
    }

    pub fn run_steps(
        &mut self,
        steps: &[StartupStep],
        mut operation: impl FnMut(StartupStep, &mut Self),
    ) {
        for step in steps {
            operation(*step, self);
        }
    }

    pub fn run_capability<T, E>(
        &mut self,
        capabilities: &[DesktopCapability],
        operation: impl FnOnce() -> Result<T, E>,
        unavailable_reason: impl FnOnce(&E) -> CapabilityUnavailableReason,
    ) -> Result<T, E> {
        let result = operation();

        match &result {
            Ok(_) => self.set_capabilities(capabilities, CapabilityStatus::available()),
            Err(error) => self.set_capabilities(
                capabilities,
                CapabilityStatus::unavailable(unavailable_reason(error)),
            ),
        }

        result
    }

    pub fn run_shell_step<T, E>(
        &mut self,
        operation: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E> {
        operation()
    }

    pub fn capabilities(&self) -> DesktopCapabilities {
        self.capabilities.clone()
    }

    fn set_capabilities(&mut self, capabilities: &[DesktopCapability], status: CapabilityStatus) {
        for capability in capabilities {
            self.capabilities.set_status(*capability, status);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_optional_failure_runs_all_later_startup_steps() {
        for failing_step in StartupStep::ALL {
            let mut coordinator = StartupCoordinator::new(DesktopSession::X11);
            let mut executed = Vec::new();

            coordinator.run_steps(&StartupStep::ALL, |step, coordinator| {
                executed.push(step);
                let succeeds = step != failing_step;

                match step {
                    StartupStep::Clipboard => run_test_capability(
                        coordinator,
                        &[
                            DesktopCapability::ClipboardRead,
                            DesktopCapability::ClipboardWrite,
                        ],
                        succeeds,
                    ),
                    StartupStep::Input => run_test_capability(
                        coordinator,
                        &[DesktopCapability::Input, DesktopCapability::Pointer],
                        succeeds,
                    ),
                    StartupStep::TargetRestoration => run_test_capability(
                        coordinator,
                        &[DesktopCapability::TargetRestoration],
                        succeeds,
                    ),
                    StartupStep::Shortcut => {
                        run_test_capability(coordinator, &[DesktopCapability::Shortcut], succeeds)
                    }
                    StartupStep::Tray => {
                        run_test_capability(coordinator, &[DesktopCapability::Tray], succeeds)
                    }
                    StartupStep::Watcher => {
                        run_test_capability(coordinator, &[DesktopCapability::Watcher], succeeds)
                    }
                    StartupStep::Window => {
                        let result = coordinator
                            .run_shell_step(|| succeeds.then_some(()).ok_or("window unavailable"));
                        assert_eq!(result.is_ok(), succeeds);
                    }
                }
            });

            assert_eq!(executed, StartupStep::ALL);
            assert_eq!(executed.last(), Some(&StartupStep::Window));

            if let Some(capability) = capability_for_step(failing_step) {
                assert_eq!(
                    coordinator.capabilities().status(capability),
                    CapabilityStatus::unavailable(
                        CapabilityUnavailableReason::InitializationFailed
                    )
                );
            }
        }
    }

    fn run_test_capability(
        coordinator: &mut StartupCoordinator,
        capabilities: &[DesktopCapability],
        succeeds: bool,
    ) {
        let result = coordinator.run_capability(
            capabilities,
            || succeeds.then_some(()).ok_or("unavailable"),
            |_| CapabilityUnavailableReason::InitializationFailed,
        );
        assert_eq!(result.is_ok(), succeeds);
    }

    fn capability_for_step(step: StartupStep) -> Option<DesktopCapability> {
        match step {
            StartupStep::Clipboard => Some(DesktopCapability::ClipboardRead),
            StartupStep::Input => Some(DesktopCapability::Input),
            StartupStep::TargetRestoration => Some(DesktopCapability::TargetRestoration),
            StartupStep::Shortcut => Some(DesktopCapability::Shortcut),
            StartupStep::Tray => Some(DesktopCapability::Tray),
            StartupStep::Watcher => Some(DesktopCapability::Watcher),
            StartupStep::Window => None,
        }
    }
}
