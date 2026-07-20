use std::sync::Mutex;

use enigo::{Enigo, Settings};

pub struct InputState {
    pub enigo: Mutex<Option<Enigo>>,
}

pub enum InputError {
    PoisonError,
    EnigoError,
}

impl InputState {
    pub fn new() -> Self {
        Self {
            enigo: Mutex::new(None),
        }
    }

    pub fn enable(&self) -> Result<(), InputError> {
        let mut guard = self.enigo.lock().map_err(|_| InputError::PoisonError)?;
        let enigo = guard.as_mut();

        if enigo.is_none() {
            let enigo = Enigo::new(&Settings::default()).map_err(|_| InputError::EnigoError)?;
            *guard = Some(enigo);
        }

        Ok(())
    }
}
