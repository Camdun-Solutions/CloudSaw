// SchedulerError — typed enum returned by every public `scheduler::*` function.
//
// Like the scanner module, errors here carry stable tags only — never raw
// SQLite text we'd later have to scrub for secrets. The fields below are
// the failure modes Contract 10 enumerates plus the bubble-up paths from
// `accounts::*` (a schedule for an account that vanishes) and `scanner::*`
// (a schedule that fires while another scan is in flight).

use crate::accounts::AccountsError;
use crate::errors::AppError;

#[derive(Debug, thiserror::Error)]
pub enum SchedulerError {
    /// Caller-side validation: empty / wrong-length / out-of-range field.
    /// Inner string is a stable field name.
    #[error("invalid input: {0}")]
    InvalidInput(&'static str),

    /// `get_schedule` / `clear_schedule` was called for an account that has
    /// no schedule row.
    #[error("schedule not found")]
    NotFound,

    /// `set_schedule` was called against an account that doesn't exist in
    /// the `accounts` table. The user must add the account first.
    #[error("account not found")]
    AccountNotFound,

    /// Bubbled from `accounts::*`.
    #[error("accounts: {0}")]
    Accounts(#[from] AccountsError),

    /// SQLite failure on the schedules table.
    #[error("db: {0}")]
    Db(String),

    /// Internal invariant violated (malformed timestamp, unknown cadence
    /// kind on read). Stable tag — never raw error text.
    #[error("internal: {0}")]
    Internal(&'static str),
}

impl SchedulerError {
    pub fn code(&self) -> &'static str {
        match self {
            SchedulerError::InvalidInput(_) => "invalid_input",
            SchedulerError::NotFound => "schedule_not_found",
            SchedulerError::AccountNotFound => "account_not_found",
            SchedulerError::Accounts(inner) => inner.code(),
            SchedulerError::Db(_) => "db_error",
            SchedulerError::Internal(_) => "internal_error",
        }
    }
}

impl From<rusqlite::Error> for SchedulerError {
    fn from(e: rusqlite::Error) -> Self {
        SchedulerError::Db(e.to_string())
    }
}

impl From<SchedulerError> for AppError {
    fn from(err: SchedulerError) -> Self {
        match err {
            SchedulerError::InvalidInput(field) => AppError::InvalidInput(field.into()),
            SchedulerError::NotFound => AppError::ScheduleNotFound,
            SchedulerError::AccountNotFound => AppError::AccountNotFound,
            SchedulerError::Accounts(inner) => AppError::from(inner),
            SchedulerError::Db(s) => AppError::Db(s),
            SchedulerError::Internal(tag) => AppError::Internal(format!("scheduler:{tag}")),
        }
    }
}
