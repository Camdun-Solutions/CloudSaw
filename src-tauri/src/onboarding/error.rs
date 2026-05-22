// Typed error enum for the onboarding wizard. Stable codes only.

use thiserror::Error;

use crate::errors::AppError;

#[derive(Debug, Error)]
pub enum OnboardingError {
    #[error("invalid input: {0}")]
    InvalidInput(&'static str),

    #[error("db: {0}")]
    Db(String),
}

impl OnboardingError {
    pub fn code(&self) -> &'static str {
        match self {
            OnboardingError::InvalidInput(_) => "invalid_input",
            OnboardingError::Db(_) => "db_error",
        }
    }
}

impl From<rusqlite::Error> for OnboardingError {
    fn from(e: rusqlite::Error) -> Self {
        OnboardingError::Db(e.to_string())
    }
}

impl From<OnboardingError> for AppError {
    fn from(e: OnboardingError) -> Self {
        match e {
            OnboardingError::InvalidInput(f) => AppError::InvalidInput(f.into()),
            OnboardingError::Db(m) => AppError::Db(m),
        }
    }
}
