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
        let result = Enigo::new(&Settings::default());

        match result {
            Ok(enigo) => {
                let Ok(mut state) = self.enigo.lock() else {
                    return Err(InputError::PoisonError);
                };
                *state = Some(enigo);
            }
            Err(_) => {
                return Err(InputError::EnigoError);
            }
        }

        Ok(())
    }
}
