// Typed error enum for the hard-delete pipeline.

use thiserror::Error;

use crate::errors::AppError;
use crate::findings::FindingsError;

#[derive(Debug, Error)]
pub enum DeletionError {
    #[error("confirmation rejected")]
    ConfirmationRejected,

    #[error("scan not found")]
    ScanNotFound,

    #[error("findings: {0}")]
    Findings(FindingsError),

    #[error("db: {0}")]
    Db(String),

    #[error("io: {0}")]
    Io(String),
}

impl From<DeletionError> for AppError {
    fn from(e: DeletionError) -> Self {
        match e {
            DeletionError::ConfirmationRejected => AppError::ConfirmationRejected,
            DeletionError::ScanNotFound => AppError::ScanNotFound,
            DeletionError::Findings(inner) => AppError::from(inner),
            DeletionError::Db(m) => AppError::Db(m),
            DeletionError::Io(m) => AppError::Io(m),
        }
    }
}
