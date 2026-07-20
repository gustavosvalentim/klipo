use std::sync::Mutex;

use enigo::{Direction, Enigo, Key, Keyboard, Settings};

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
    #[cfg(target_os = "macos")]
    let mod_key = Key::Meta;

    #[cfg(not(target_os = "macos"))]
    let mod_key = Key::Control;

    if let Err(e) = enigo.key(mod_key, Direction::Press) {
        return Err(InputError::InputSimError(e));
    }

    let _ = enigo.key(Key::Unicode('v'), Direction::Click);

    if let Err(e) = enigo.key(mod_key, Direction::Release) {
        return Err(InputError::InputSimError(e));
    }

    Ok(())
}

pub struct InputState {
    pub enigo: Mutex<Option<Enigo>>,
}

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
