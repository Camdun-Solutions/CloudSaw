// AccountsError — typed enum returned by every public `accounts::*` function.
//
// Each variant maps to a stable, enumerated IPC error code via `code()` and
// folds into `AppError` for serialization. Like AuthError, no raw error
// chain ever crosses IPC.
//
// The two duplicate-detection variants (`DuplicateLabel`,
// `DuplicateAwsAccountId`) are kept distinct so the UI can localize the
// remediation copy precisely — "a different label already uses that account"
// vs. "another account is using that label".

use crate::auth::AuthError;
use crate::errors::AppError;

#[derive(Debug, thiserror::Error)]
pub enum AccountsError {
    /// Label or profile-name failed validation (length, charset, emptiness).
    #[error("invalid input: {0}")]
    InvalidInput(&'static str),

    /// Caller asked for an account ID that isn't in the `accounts` table.
    #[error("account not found")]
    NotFound,

    /// Add/update would produce a row whose AWS account ID matches an
    /// existing row. Per Contract 04 §Edge Cases, we surface this rather
    /// than silently allowing duplicates.
    #[error("duplicate aws account id")]
    DuplicateAwsAccountId,

    /// Add/update would produce a row whose label matches an existing
    /// (different) account. Labels are unique so the UI list is unambiguous.
    #[error("duplicate label")]
    DuplicateLabel,

    /// Update with profile change would change the underlying AWS account
    /// ID — that's a different account, not an edit. The user is told to
    /// remove and re-add instead.
    #[error("account id mismatch")]
    AwsAccountIdMismatch,

    /// `get_caller_identity` failed during add/update verification. The
    /// inner `AuthError` carries the stable reason code; nothing in here
    /// includes raw SDK text or credential material.
    #[error("verification failed: {0}")]
    Verification(#[from] AuthError),

    /// SQLite operation failed. The wrapped string is the rusqlite Display,
    /// already free of credential material — only error types/codes from
    /// SQLite itself, which CloudSaw produces.
    #[error("db: {0}")]
    Db(String),

    /// Internal invariant violated (e.g. malformed timestamp pulled from the
    /// DB). Short stable tag — never raw error text.
    #[error("internal: {0}")]
    Internal(&'static str),
}

impl AccountsError {
    pub fn code(&self) -> &'static str {
        match self {
            AccountsError::InvalidInput(_) => "invalid_input",
            AccountsError::NotFound => "account_not_found",
            AccountsError::DuplicateAwsAccountId => "duplicate_aws_account_id",
            AccountsError::DuplicateLabel => "duplicate_label",
            AccountsError::AwsAccountIdMismatch => "aws_account_id_mismatch",
            AccountsError::Verification(inner) => inner.code(),
            AccountsError::Db(_) => "db_error",
            AccountsError::Internal(_) => "internal_error",
        }
    }
}

impl From<rusqlite::Error> for AccountsError {
    fn from(e: rusqlite::Error) -> Self {
        AccountsError::Db(e.to_string())
    }
}

impl From<AccountsError> for AppError {
    fn from(err: AccountsError) -> Self {
        match err {
            AccountsError::InvalidInput(field) => AppError::InvalidInput(field.into()),
            AccountsError::NotFound => AppError::AccountNotFound,
            AccountsError::DuplicateAwsAccountId => AppError::DuplicateAwsAccountId,
            AccountsError::DuplicateLabel => AppError::DuplicateLabel,
            AccountsError::AwsAccountIdMismatch => AppError::AccountIdMismatch,
            AccountsError::Verification(inner) => AppError::from(inner),
            AccountsError::Db(s) => AppError::Db(s),
            AccountsError::Internal(tag) => AppError::Internal(format!("accounts:{tag}")),
        }
    }
}
