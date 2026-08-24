#[cfg(target_os = "macos")]
use tauri::window::{Effect, EffectState, EffectsBuilder};
use tauri::{Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, Window, WindowEvent};

use crate::state::AppState;

const MAIN_WINDOW_LABEL: &str = "main";
const SETTINGS_WINDOW_LABEL: &str = "settings";

pub struct Settings {
    pub width: f64,
    pub height: f64,
    pub transparent: bool,
    pub decorations: bool,
}

pub(crate) const PICKER_WIDTH: f64 = 250.0;
pub(crate) const PICKER_HEIGHT: f64 = 350.0;

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

pub fn create_klipo_window(
    app: &tauri::AppHandle,
    settings: Settings,
) -> Result<WebviewWindow, WindowError> {
    let window_builder = WebviewWindowBuilder::new(app, MAIN_WINDOW_LABEL, WebviewUrl::default())
        .inner_size(settings.width, settings.height)
        .decorations(settings.decorations)
        .transparent(settings.transparent)
        .always_on_top(true)
        .visible(false)
        .visible_on_all_workspaces(true)
        .shadow(false);

    #[cfg(target_os = "macos")]
    let window_builder = window_builder
        // `Menu` matches a macOS popup menu more closely than `Popover`.
        // Its radius clips the native backdrop too, rather than leaving a
        // square vibrancy layer behind the rounded web content.
        .effects(
            EffectsBuilder::new()
                .effect(Effect::Menu)
                .state(EffectState::Active)
                .radius(11.0)
                .build(),
        );

    let window = window_builder.build();

    let window = match window {
        Ok(window) => window,
        Err(e) => return Err(WindowError::TauriError(e)),
    };

    Ok(window)
}

pub fn create_picker_window(app: &tauri::AppHandle) -> Result<WebviewWindow, WindowError> {
    create_klipo_window(
        app,
        Settings {
            width: PICKER_WIDTH,
            height: PICKER_HEIGHT,
            transparent: cfg!(target_os = "macos"),
            decorations: false,
        },
    )
}

pub fn get_main_window(app: &tauri::AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(MAIN_WINDOW_LABEL)
}

#[cfg(target_os = "linux")]
pub fn show_picker_window(app: &tauri::AppHandle) -> Result<(), tauri::Error> {
    let window = get_main_window(app).ok_or(tauri::Error::WindowNotFound)?;

    window.show()?;
    window.set_focus()
}

pub fn show_settings_window(app: &tauri::AppHandle) -> Result<(), tauri::Error> {
    let window = match app.get_webview_window(SETTINGS_WINDOW_LABEL) {
        Some(window) => window,
        None => WebviewWindowBuilder::new(app, SETTINGS_WINDOW_LABEL, WebviewUrl::default())
            .title("Klipo Settings")
            .inner_size(560.0, 510.0)
            .min_inner_size(560.0, 510.0)
            .resizable(false)
            .build()?,
    };
    window.show()?;
    window.set_focus()
}

pub fn window_events_handler(window: &Window, event: &WindowEvent) {
    if window.label() == SETTINGS_WINDOW_LABEL {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = window.hide();
        }
        return;
    }
    if let WindowEvent::Focused(focused) = event {
        if !focused {
            let _ = window.hide();
        }
    }
}

#[derive(Debug)]
pub enum FocusError {
    PlatformUnsupported,
    StatePoisoned,
    FocusedWindowUnavailable,
}

/// Opaque platform focus target. Native process identifiers remain private to
/// this module so callers can only capture, restore, or compare focus.
pub(crate) struct FocusTarget {
    target: FocusTargetKind,
}

enum FocusTargetKind {
    Empty,
    #[cfg(target_os = "macos")]
    MacosProcess(i32),
    #[cfg(target_os = "linux")]
    X11(x11::FocusTarget),
}

impl Default for FocusTarget {
    fn default() -> Self {
        Self {
            target: FocusTargetKind::Empty,
        }
    }
}

impl FocusTarget {
    pub(crate) fn empty() -> Self {
        Self::default()
    }

    fn current() -> Result<Self, FocusError> {
        #[cfg(target_os = "macos")]
        {
            let process_id = focused_process_id().ok_or(FocusError::FocusedWindowUnavailable)?;
            Ok(Self {
                target: FocusTargetKind::MacosProcess(process_id),
            })
        }

        #[cfg(target_os = "linux")]
        {
            if crate::desktop::detect_session() == crate::desktop::DesktopSession::X11 {
                x11::capture().map(|target| Self {
                    target: FocusTargetKind::X11(target),
                })
            } else {
                Err(FocusError::PlatformUnsupported)
            }
        }

        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            Err(FocusError::PlatformUnsupported)
        }
    }

    fn restore(&self) -> Result<(), FocusError> {
        match &self.target {
            FocusTargetKind::Empty => Err(FocusError::FocusedWindowUnavailable),
            #[cfg(target_os = "macos")]
            FocusTargetKind::MacosProcess(process_id) => {
                if activate_process(*process_id) {
                    Ok(())
                } else {
                    Err(FocusError::PlatformUnsupported)
                }
            }
            #[cfg(target_os = "linux")]
            FocusTargetKind::X11(target) => x11::restore(target),
        }
    }
}

impl std::fmt::Display for FocusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FocusError::PlatformUnsupported => write!(f, "Platform unsupported"),
            FocusError::StatePoisoned => write!(f, "Focused window state poisoned"),
            FocusError::FocusedWindowUnavailable => write!(f, "Focused window unavailable"),
        }
    }
}

pub fn capture_focused_window(state: &AppState) -> Result<(), FocusError> {
    let mut focus_target = state
        .focus_target
        .lock()
        .map_err(|_| FocusError::StatePoisoned)?;

    replace_focus_target(&mut focus_target, FocusTarget::current())
}

fn replace_focus_target(
    focus_target: &mut FocusTarget,
    captured_target: Result<FocusTarget, FocusError>,
) -> Result<(), FocusError> {
    match captured_target {
        Ok(captured_target) => {
            *focus_target = captured_target;
            Ok(())
        }
        Err(error_value) => {
            *focus_target = FocusTarget::empty();
            Err(error_value)
        }
    }
}

pub fn restore_focused_window(state: &AppState) -> Result<(), FocusError> {
    let focus_target = state
        .focus_target
        .lock()
        .map_err(|_| FocusError::StatePoisoned)?;

    focus_target.restore()
}

pub fn is_klipo_focused() -> bool {
    #[cfg(target_os = "macos")]
    {
        is_current_process(focused_process_id(), std::process::id())
    }

    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

fn is_current_process(focused_process_id: Option<i32>, current_process_id: u32) -> bool {
    focused_process_id == i32::try_from(current_process_id).ok()
}

fn focused_process_id() -> Option<i32> {
    #[cfg(target_os = "macos")]
    {
        use crate::window::macos::focused_process_id;

        focused_process_id()
    }

    #[cfg(not(target_os = "macos"))]
    {
        log::warn!(platform = std::env::consts::OS; "Focused window lookup is not implemented");
        None
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn supports_target_restoration() -> bool {
    x11::probe().is_ok()
}

fn activate_process(process_id: i32) -> bool {
    #[cfg(target_os = "macos")]
    {
        use crate::window::macos::activate_process;

        activate_process(process_id)
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = process_id;
        log::warn!(platform = std::env::consts::OS; "Focused window restoration is not implemented");
        false
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWorkspace};

    pub fn activate_process(pid: i32) -> bool {
        let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) else {
            return false;
        };

        app.activateWithOptions(NSApplicationActivationOptions::empty())
    }

    pub fn focused_process_id() -> Option<i32> {
        let workspace = NSWorkspace::sharedWorkspace();
        let app = workspace.frontmostApplication();

        Some(app?.processIdentifier())
    }
}

/// X11 window identifiers and EWMH protocol details stay in this adapter.
/// The rest of the application can only hold an opaque target and request its
/// restoration, which prevents process identifiers from becoming identity.
#[cfg(any(target_os = "linux", test))]
#[cfg_attr(test, allow(dead_code))]
mod x11 {
    use std::time::Duration;

    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{
        Atom, AtomEnum, ClientMessageEvent, ConnectionExt, EventMask, MapState, Window,
    };
    use x11rb::rust_connection::RustConnection;

    use super::FocusError;

    const ACTIVE_WINDOW: &[u8] = b"_NET_ACTIVE_WINDOW";
    const ACTIVATION_RETRIES: usize = 10;

    #[derive(Debug)]
    pub(super) struct FocusTarget {
        window: Window,
    }

    trait Protocol {
        fn active_window(&mut self) -> Result<Option<Window>, ()>;
        fn target_is_viewable(&mut self, target: Window) -> Result<bool, ()>;
        fn request_activation(&mut self, request: ActivationRequest) -> Result<(), ()>;
    }

    trait EwmhProtocol {
        fn existing_atom(&mut self, name: &[u8]) -> Result<Option<Atom>, ()>;
        fn atom_list_property(&mut self, property: Atom) -> Result<Option<AtomListProperty>, ()>;
    }

    struct AtomListProperty {
        property_type: Atom,
        format: u8,
        atoms: Vec<Atom>,
    }

    #[derive(Debug, PartialEq, Eq)]
    struct ActivationRequest {
        target: Window,
        source: u32,
        timestamp: u32,
    }

    impl ActivationRequest {
        fn for_target(target: Window) -> Self {
            Self {
                target,
                // EWMH source indication: a regular application request.
                source: 1,
                // A global shortcut has no X event timestamp to forward.
                timestamp: 0,
            }
        }
    }

    pub(super) fn capture() -> Result<FocusTarget, FocusError> {
        let mut connection =
            NativeProtocol::connect().map_err(|_| FocusError::PlatformUnsupported)?;
        validate_active_window_support(&mut connection)
            .map_err(|_| FocusError::PlatformUnsupported)?;
        capture_with(&mut connection)
    }

    pub(super) fn restore(target: &FocusTarget) -> Result<(), FocusError> {
        let mut connection =
            NativeProtocol::connect().map_err(|_| FocusError::PlatformUnsupported)?;
        validate_active_window_support(&mut connection)
            .map_err(|_| FocusError::PlatformUnsupported)?;
        restore_with(&mut connection, target, || {
            std::thread::sleep(Duration::from_millis(20))
        })
    }

    pub(super) fn probe() -> Result<(), ()> {
        let mut connection = NativeProtocol::connect()?;
        validate_active_window_support(&mut connection)
    }

    fn validate_active_window_support(protocol: &mut impl EwmhProtocol) -> Result<(), ()> {
        let active_window_atom = protocol.existing_atom(ACTIVE_WINDOW)?.ok_or(())?;
        let supported_atom = protocol.existing_atom(b"_NET_SUPPORTED")?.ok_or(())?;
        let supported_property = protocol.atom_list_property(supported_atom)?.ok_or(())?;

        if supported_property.property_type == u32::from(AtomEnum::ATOM)
            && supported_property.format == 32
            && supported_property.atoms.contains(&active_window_atom)
        {
            Ok(())
        } else {
            Err(())
        }
    }

    fn capture_with(protocol: &mut impl Protocol) -> Result<FocusTarget, FocusError> {
        let target = protocol
            .active_window()
            .map_err(|_| FocusError::FocusedWindowUnavailable)?
            .ok_or(FocusError::FocusedWindowUnavailable)?;

        if protocol
            .target_is_viewable(target)
            .map_err(|_| FocusError::FocusedWindowUnavailable)?
        {
            Ok(FocusTarget { window: target })
        } else {
            Err(FocusError::FocusedWindowUnavailable)
        }
    }

    fn restore_with(
        protocol: &mut impl Protocol,
        target: &FocusTarget,
        mut wait: impl FnMut(),
    ) -> Result<(), FocusError> {
        let exists = protocol
            .target_is_viewable(target.window)
            .map_err(|_| FocusError::FocusedWindowUnavailable)?;

        if exists {
            protocol
                .request_activation(ActivationRequest::for_target(target.window))
                .map_err(|_| FocusError::PlatformUnsupported)?;

            let mut restored = false;
            for attempt in 0..ACTIVATION_RETRIES {
                let active_window = protocol
                    .active_window()
                    .map_err(|_| FocusError::FocusedWindowUnavailable)?;

                if active_window == Some(target.window) {
                    restored = protocol
                        .target_is_viewable(target.window)
                        .map_err(|_| FocusError::FocusedWindowUnavailable)?;

                    if restored {
                        break;
                    }
                }

                if attempt + 1 < ACTIVATION_RETRIES {
                    wait();
                }
            }

            if restored {
                Ok(())
            } else {
                Err(FocusError::FocusedWindowUnavailable)
            }
        } else {
            Err(FocusError::FocusedWindowUnavailable)
        }
    }

    struct NativeProtocol {
        connection: RustConnection,
        root: Window,
        active_window_atom: u32,
    }

    impl NativeProtocol {
        fn connect() -> Result<Self, ()> {
            let (connection, screen_index) = RustConnection::connect(None).map_err(|_| ())?;
            let root = connection
                .setup()
                .roots
                .get(screen_index)
                .map(|screen| screen.root)
                .ok_or(())?;
            let active_window_atom = existing_atom(&connection, ACTIVE_WINDOW)?.ok_or(())?;

            Ok(Self {
                connection,
                root,
                active_window_atom,
            })
        }
    }

    impl Protocol for NativeProtocol {
        fn active_window(&mut self) -> Result<Option<Window>, ()> {
            let reply = self
                .connection
                .get_property(
                    false,
                    self.root,
                    self.active_window_atom,
                    AtomEnum::WINDOW,
                    0,
                    1,
                )
                .map_err(|_| ())?
                .reply()
                .map_err(|_| ())?;

            Ok(reply.value32().and_then(|mut values| values.next()))
        }

        fn target_is_viewable(&mut self, target: Window) -> Result<bool, ()> {
            self.connection
                .get_window_attributes(target)
                .map_err(|_| ())?
                .reply()
                .map(|attributes| attributes.map_state == MapState::VIEWABLE)
                .map_err(|_| ())
        }

        fn request_activation(&mut self, request: ActivationRequest) -> Result<(), ()> {
            let event = ClientMessageEvent::new(
                32,
                request.target,
                self.active_window_atom,
                [request.source, request.timestamp, 0, 0, 0],
            );
            let request = self
                .connection
                .send_event(
                    false,
                    self.root,
                    EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                    event,
                )
                .map_err(|_| ())?;
            self.connection.flush().map_err(|_| ())?;
            request.check().map_err(|_| ())
        }
    }

    impl EwmhProtocol for NativeProtocol {
        fn existing_atom(&mut self, name: &[u8]) -> Result<Option<Atom>, ()> {
            existing_atom(&self.connection, name)
        }

        fn atom_list_property(&mut self, property: Atom) -> Result<Option<AtomListProperty>, ()> {
            let reply = self
                .connection
                .get_property(false, self.root, property, AtomEnum::ATOM, 0, u32::MAX)
                .map_err(|_| ())?
                .reply()
                .map_err(|_| ())?;

            let atoms = reply
                .value32()
                .map(|values| values.collect())
                .unwrap_or_default();

            Ok(Some(AtomListProperty {
                property_type: reply.type_,
                format: reply.format,
                atoms,
            }))
        }
    }

    fn existing_atom(connection: &RustConnection, name: &[u8]) -> Result<Option<Atom>, ()> {
        let atom = connection
            .intern_atom(true, name)
            .map_err(|_| ())?
            .reply()
            .map_err(|_| ())?
            .atom;

        Ok((atom != 0).then_some(atom))
    }

    #[cfg(test)]
    mod tests {
        use std::collections::VecDeque;

        use super::*;

        struct FakeProtocol {
            active_windows: Vec<Option<Window>>,
            target_viewability: VecDeque<bool>,
            activation_allowed: bool,
            activation_requests: Vec<ActivationRequest>,
        }

        impl FakeProtocol {
            fn new(active_windows: impl IntoIterator<Item = Option<Window>>) -> Self {
                Self {
                    active_windows: active_windows.into_iter().collect(),
                    target_viewability: VecDeque::new(),
                    activation_allowed: true,
                    activation_requests: Vec::new(),
                }
            }

            fn with_target_viewability(
                active_windows: impl IntoIterator<Item = Option<Window>>,
                target_viewability: impl IntoIterator<Item = bool>,
            ) -> Self {
                Self {
                    active_windows: active_windows.into_iter().collect(),
                    target_viewability: target_viewability.into_iter().collect(),
                    ..Self::new([])
                }
            }
        }

        impl Protocol for FakeProtocol {
            fn active_window(&mut self) -> Result<Option<Window>, ()> {
                if self.active_windows.is_empty() {
                    Ok(None)
                } else {
                    Ok(self.active_windows.remove(0))
                }
            }

            fn target_is_viewable(&mut self, _target: Window) -> Result<bool, ()> {
                Ok(self.target_viewability.pop_front().unwrap_or(true))
            }

            fn request_activation(&mut self, request: ActivationRequest) -> Result<(), ()> {
                self.activation_requests.push(request);
                if self.activation_allowed {
                    Ok(())
                } else {
                    Err(())
                }
            }
        }

        struct FakeEwmhProtocol {
            active_window_atom: Option<Atom>,
            supported_atom: Option<Atom>,
            supported_property: Option<AtomListProperty>,
        }

        impl EwmhProtocol for FakeEwmhProtocol {
            fn existing_atom(&mut self, name: &[u8]) -> Result<Option<Atom>, ()> {
                match name {
                    ACTIVE_WINDOW => Ok(self.active_window_atom),
                    b"_NET_SUPPORTED" => Ok(self.supported_atom),
                    _ => Err(()),
                }
            }

            fn atom_list_property(
                &mut self,
                property: Atom,
            ) -> Result<Option<AtomListProperty>, ()> {
                if Some(property) == self.supported_atom {
                    Ok(self.supported_property.take())
                } else {
                    Err(())
                }
            }
        }

        #[test]
        fn captures_the_exact_active_window_without_a_process_identifier() {
            let mut protocol = FakeProtocol::new([Some(42)]);

            let target = capture_with(&mut protocol).unwrap();

            assert_eq!(target.window, 42);
        }

        #[test]
        fn restores_the_exact_captured_window_and_verifies_the_active_property() {
            let mut protocol = FakeProtocol::new([Some(7), Some(42)]);
            let target = FocusTarget { window: 42 };
            let mut waits = 0;

            let result = restore_with(&mut protocol, &target, || waits += 1);

            assert!(result.is_ok());
            assert_eq!(
                protocol.activation_requests,
                [ActivationRequest {
                    target: 42,
                    source: 1,
                    timestamp: 0,
                }]
            );
            assert_eq!(waits, 1);
        }

        #[test]
        fn reports_manual_paste_when_the_captured_window_was_closed() {
            let mut protocol = FakeProtocol::new([]);
            protocol.target_viewability = VecDeque::from([false]);
            let target = FocusTarget { window: 42 };

            let result = restore_with(&mut protocol, &target, || {});

            assert!(matches!(result, Err(FocusError::FocusedWindowUnavailable)));
            assert!(protocol.activation_requests.is_empty());
        }

        #[test]
        fn does_not_treat_a_sent_activation_event_as_a_success() {
            let mut protocol = FakeProtocol::new(std::iter::repeat_n(Some(7), ACTIVATION_RETRIES));
            let target = FocusTarget { window: 42 };

            let result = restore_with(&mut protocol, &target, || {});

            assert!(matches!(result, Err(FocusError::FocusedWindowUnavailable)));
            assert_eq!(protocol.activation_requests.len(), 1);
        }

        #[test]
        fn reports_manual_paste_when_the_activation_request_is_rejected() {
            let mut protocol = FakeProtocol::new([]);
            protocol.activation_allowed = false;
            let target = FocusTarget { window: 42 };

            let result = restore_with(&mut protocol, &target, || {});

            assert!(matches!(result, Err(FocusError::PlatformUnsupported)));
            assert_eq!(protocol.activation_requests.len(), 1);
        }

        #[test]
        fn reports_manual_paste_when_the_target_closes_before_focus_verification() {
            let mut protocol = FakeProtocol::with_target_viewability([Some(42)], [true, false]);
            let target = FocusTarget { window: 42 };

            let result = restore_with(&mut protocol, &target, || {});

            assert!(matches!(result, Err(FocusError::FocusedWindowUnavailable)));
        }

        #[test]
        fn validates_existing_e_w_m_h_active_window_support() {
            let mut protocol = FakeEwmhProtocol {
                active_window_atom: Some(42),
                supported_atom: Some(7),
                supported_property: Some(AtomListProperty {
                    property_type: AtomEnum::ATOM.into(),
                    format: 32,
                    atoms: vec![42],
                }),
            };

            assert!(validate_active_window_support(&mut protocol).is_ok());
        }

        #[test]
        fn rejects_missing_or_malformed_e_w_m_h_support() {
            let mut missing_active_window = FakeEwmhProtocol {
                active_window_atom: None,
                supported_atom: Some(7),
                supported_property: Some(AtomListProperty {
                    property_type: AtomEnum::ATOM.into(),
                    format: 32,
                    atoms: vec![42],
                }),
            };
            let mut malformed_supported_property = FakeEwmhProtocol {
                active_window_atom: Some(42),
                supported_atom: Some(7),
                supported_property: Some(AtomListProperty {
                    property_type: AtomEnum::WINDOW.into(),
                    format: 8,
                    atoms: vec![42],
                }),
            };
            let mut unsupported_active_window = FakeEwmhProtocol {
                active_window_atom: Some(42),
                supported_atom: Some(7),
                supported_property: Some(AtomListProperty {
                    property_type: AtomEnum::ATOM.into(),
                    format: 32,
                    atoms: vec![9],
                }),
            };

            assert!(validate_active_window_support(&mut missing_active_window).is_err());
            assert!(validate_active_window_support(&mut malformed_supported_property).is_err());
            assert!(validate_active_window_support(&mut unsupported_active_window).is_err());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_klipo_as_the_focused_process_without_exposing_its_identifier() {
        assert!(is_current_process(Some(42), 42));
        assert!(!is_current_process(Some(41), 42));
        assert!(!is_current_process(None, 42));
    }

    #[test]
    fn does_not_restore_when_no_focus_target_was_captured() {
        let result = FocusTarget::empty().restore();

        assert!(matches!(result, Err(FocusError::FocusedWindowUnavailable)));
    }

    #[test]
    fn failed_capture_clears_the_previous_target_before_manual_paste() {
        let mut focus_target = FocusTarget::empty();

        let result =
            replace_focus_target(&mut focus_target, Err(FocusError::FocusedWindowUnavailable));

        assert!(matches!(result, Err(FocusError::FocusedWindowUnavailable)));
        assert!(matches!(focus_target.target, FocusTargetKind::Empty));
    }
}
