// Typed error enum for the retention engine. Collapses foreign errors to
// short strings — same pattern every other module follows.

use thiserror::Error;

use crate::errors::AppError;

#[derive(Debug, Error)]
pub enum RetentionError {
    #[error("invalid input: {0}")]
    InvalidInput(&'static str),

    #[error("db: {0}")]
    Db(String),

    #[error("io: {0}")]
    Io(String),
}

impl From<rusqlite::Error> for RetentionError {
    fn from(e: rusqlite::Error) -> Self {
        RetentionError::Db(e.to_string())
    }
}

impl From<std::io::Error> for RetentionError {
    fn from(e: std::io::Error) -> Self {
        RetentionError::Io(e.to_string())
    }
}

impl From<RetentionError> for AppError {
    fn from(e: RetentionError) -> Self {
        match e {
            RetentionError::InvalidInput(f) => AppError::InvalidInput(f.into()),
            RetentionError::Db(m) => AppError::Db(m),
            RetentionError::Io(m) => AppError::Io(m),
        }
    }
}
