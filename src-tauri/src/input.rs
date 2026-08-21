use std::sync::Mutex;

use enigo::{Direction, Enigo, Key, Keyboard, Settings};

use crate::desktop::DesktopSession;

trait KeyInput {
    fn key(&mut self, key: Key, direction: Direction) -> Result<(), enigo::InputError>;
}

impl KeyInput for Enigo {
    fn key(&mut self, key: Key, direction: Direction) -> Result<(), enigo::InputError> {
        Keyboard::key(self, key, direction)
    }
}

#[derive(Debug)]
pub enum InputError {
    InputSimError(enigo::InputError),
}

impl std::fmt::Display for InputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InputError::InputSimError(e) => write!(f, "Input simulation error: {e}"),
        }
    }
}

pub fn simulate_paste_input(enigo: &mut Enigo) -> Result<(), InputError> {
    simulate_paste_input_with(enigo, paste_modifier())
}

fn paste_modifier() -> Key {
    #[cfg(target_os = "macos")]
    {
        Key::Meta
    }

    #[cfg(not(target_os = "macos"))]
    {
        Key::Control
    }
}

fn simulate_paste_input_with(input: &mut impl KeyInput, mod_key: Key) -> Result<(), InputError> {
    if let Err(e) = input.key(mod_key, Direction::Press) {
        return Err(InputError::InputSimError(e));
    }

    let click_result = input.key(Key::Unicode('v'), Direction::Click);
    let release_result = input.key(mod_key, Direction::Release);

    match (click_result, release_result) {
        (Err(error), _) | (_, Err(error)) => Err(InputError::InputSimError(error)),
        (Ok(()), Ok(())) => Ok(()),
    }
}

pub fn supports_input(session: DesktopSession) -> bool {
    supports_input_on(std::env::consts::OS, session)
}

fn supports_input_on(platform: &str, session: DesktopSession) -> bool {
    match platform {
        "macos" => true,
        "linux" => matches!(session, DesktopSession::X11),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    struct FakeKeyInput {
        calls: Vec<(Key, Direction)>,
        results: VecDeque<Result<(), enigo::InputError>>,
    }

    impl FakeKeyInput {
        fn new(results: impl IntoIterator<Item = Result<(), enigo::InputError>>) -> Self {
            Self {
                calls: Vec::new(),
                results: results.into_iter().collect(),
            }
        }
    }

    impl KeyInput for FakeKeyInput {
        fn key(&mut self, key: Key, direction: Direction) -> Result<(), enigo::InputError> {
            self.calls.push((key, direction));
            self.results
                .pop_front()
                .expect("a result for every key call")
        }
    }

    fn input_error(message: &'static str) -> enigo::InputError {
        enigo::InputError::Simulate(message)
    }

    fn expected_calls(mod_key: Key) -> Vec<(Key, Direction)> {
        vec![
            (mod_key, Direction::Press),
            (Key::Unicode('v'), Direction::Click),
            (mod_key, Direction::Release),
        ]
    }

    #[test]
    fn uses_the_platform_paste_modifier() {
        #[cfg(target_os = "macos")]
        let expected = Key::Meta;

        #[cfg(not(target_os = "macos"))]
        let expected = Key::Control;

        assert_eq!(paste_modifier(), expected);
    }

    #[test]
    fn press_failure_does_not_attempt_later_input() {
        let mut input = FakeKeyInput::new([Err(input_error("press"))]);

        let result = simulate_paste_input_with(&mut input, Key::Meta);

        assert!(matches!(
            result,
            Err(InputError::InputSimError(enigo::InputError::Simulate(
                "press"
            )))
        ));
        assert_eq!(input.calls, vec![(Key::Meta, Direction::Press)]);
    }

    #[test]
    fn click_failure_is_returned_after_releasing_modifier() {
        let mut input = FakeKeyInput::new([Ok(()), Err(input_error("click")), Ok(())]);

        let result = simulate_paste_input_with(&mut input, Key::Meta);

        assert!(matches!(
            result,
            Err(InputError::InputSimError(enigo::InputError::Simulate(
                "click"
            )))
        ));
        assert_eq!(input.calls, expected_calls(Key::Meta));
    }

    #[test]
    fn click_failure_remains_primary_when_release_also_fails() {
        let mut input = FakeKeyInput::new([
            Ok(()),
            Err(input_error("click")),
            Err(input_error("release")),
        ]);

        let result = simulate_paste_input_with(&mut input, Key::Meta);

        assert!(matches!(
            result,
            Err(InputError::InputSimError(enigo::InputError::Simulate(
                "click"
            )))
        ));
        assert_eq!(input.calls, expected_calls(Key::Meta));
    }

    #[test]
    fn release_failure_is_returned_when_press_and_click_succeed() {
        let mut input = FakeKeyInput::new([Ok(()), Ok(()), Err(input_error("release"))]);

        let result = simulate_paste_input_with(&mut input, Key::Meta);

        assert!(matches!(
            result,
            Err(InputError::InputSimError(enigo::InputError::Simulate(
                "release"
            )))
        ));
        assert_eq!(input.calls, expected_calls(Key::Meta));
    }

    #[test]
    fn successful_paste_input_presses_clicks_and_releases_modifier() {
        let mut input = FakeKeyInput::new([Ok(()), Ok(()), Ok(())]);

        let result = simulate_paste_input_with(&mut input, Key::Meta);

        assert!(result.is_ok());
        assert_eq!(input.calls, expected_calls(Key::Meta));
    }

    #[test]
    fn limits_linux_input_to_x11_but_preserves_macos_support() {
        assert!(supports_input_on("linux", DesktopSession::X11));
        assert!(!supports_input_on("linux", DesktopSession::Wayland));
        assert!(!supports_input_on("linux", DesktopSession::Unknown));
        assert!(supports_input_on("macos", DesktopSession::Unknown));
    }
}

pub struct InputState {
    pub enigo: Mutex<Option<Enigo>>,
}

#[derive(Debug)]
pub enum InputStateError {
    PoisonError,
    EnigoError,
}

impl InputState {
    pub fn new() -> Self {
        Self {
            enigo: Mutex::new(None),
        }
    }

    pub fn enable(&self) -> Result<(), InputStateError> {
        let mut guard = self
            .enigo
            .lock()
            .map_err(|_| InputStateError::PoisonError)?;
        let enigo = guard.as_mut();

        if enigo.is_none() {
            let enigo =
                Enigo::new(&Settings::default()).map_err(|_| InputStateError::EnigoError)?;
            *guard = Some(enigo);
        }

        Ok(())
    }
}
