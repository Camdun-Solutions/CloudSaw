// Typed error enum for the event-log surface. Mirrors the pattern used by
// every other module — collapse foreign error sources to a short string,
// keep the stable variants the IPC layer translates into error codes.

use thiserror::Error;

use crate::errors::AppError;

#[derive(Debug, Error)]
pub enum EventLogError {
    #[error("invalid input: {0}")]
    InvalidInput(&'static str),

    #[error("db: {0}")]
    Db(String),

    #[error("io: {0}")]
    Io(String),
}

impl From<rusqlite::Error> for EventLogError {
    fn from(e: rusqlite::Error) -> Self {
        EventLogError::Db(e.to_string())
    }
}

impl From<EventLogError> for AppError {
    fn from(e: EventLogError) -> Self {
        match e {
            EventLogError::InvalidInput(field) => AppError::InvalidInput(field.into()),
            EventLogError::Db(msg) => AppError::Db(msg),
            EventLogError::Io(msg) => AppError::Io(msg),
        }
    }
}
