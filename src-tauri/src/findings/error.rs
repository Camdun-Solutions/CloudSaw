// FindingsError — typed enum returned by every public `findings::*` function.
//
// Each variant maps to a stable IPC error code via `code()` and folds into
// `AppError` for serialization. CLAUDE.md §4.2: no raw scanner stderr, no
// raw SDK text, no credential material — these errors only carry stable
// tags or filesystem strings.

use crate::accounts::AccountsError;
use crate::errors::AppError;
use crate::scanner::ScannerError;

#[derive(Debug, thiserror::Error)]
pub enum FindingsError {
    /// Caller-side validation failure (empty scan_id, malformed account_id,
    /// negative limit, …). The inner string is a stable field name.
    #[error("invalid input: {0}")]
    InvalidInput(&'static str),

    /// The scan_id supplied to `parse_and_store` / `list_findings` /
    /// `get_scan` / `delete_scan` does not exist in the scans table.
    #[error("scan not found")]
    ScanNotFound,

    /// `get_finding` was called for a finding_id that isn't in the table.
    #[error("finding not found")]
    FindingNotFound,

    /// `parse_and_store` was called on a scan that has no raw-output path
    /// recorded — typically because the scan failed before reaching the
    /// parsing stage. The orchestrator should never do this, but we guard
    /// against it so a stale frontend retry doesn't crash.
    #[error("scan has no raw output")]
    NoRawOutput,

    /// The scanner output file disappeared between scan completion and the
    /// parse attempt. Distinct from `Io` so the UI can surface a more
    /// helpful message.
    #[error("raw output file missing")]
    RawOutputMissing,

    /// `raw-scout.json` failed to deserialize. The inner string is the
    /// `serde_json::Error::Display` form, which contains line/column info
    /// and the source error kind but no credential material.
    #[error("malformed scanner output: {0}")]
    ParseMalformed(String),

    /// `raw-scout.json` deserialized but its top-level `account_id` did not
    /// match the scan's account_id. Treated as a hard error — we never
    /// store findings under the wrong partition key.
    #[error("scan account mismatch")]
    AccountMismatch,

    /// Bubbled from scanner-storage helpers when we look up the scan row.
    #[error("scanner: {0}")]
    Scanner(#[from] ScannerError),

    /// Bubbled from accounts validation (account_id grammar checks).
    #[error("accounts: {0}")]
    Accounts(#[from] AccountsError),

    /// Filesystem failure reading `raw-scout.json`.
    #[error("io: {0}")]
    Io(String),

    /// SQLite operation failed.
    #[error("db: {0}")]
    Db(String),

    /// Internal invariant violated. Stable source-code tag, never raw text
    /// from a third party.
    #[error("internal: {0}")]
    Internal(&'static str),
}

impl FindingsError {
    pub fn code(&self) -> &'static str {
        match self {
            FindingsError::InvalidInput(_) => "invalid_input",
            FindingsError::ScanNotFound => "scan_not_found",
            FindingsError::FindingNotFound => "finding_not_found",
            FindingsError::NoRawOutput => "findings_no_raw_output",
            FindingsError::RawOutputMissing => "findings_raw_output_missing",
            FindingsError::ParseMalformed(_) => "findings_parse_malformed",
            FindingsError::AccountMismatch => "findings_account_mismatch",
            FindingsError::Scanner(inner) => inner.code(),
            FindingsError::Accounts(inner) => inner.code(),
            FindingsError::Io(_) => "io_error",
            FindingsError::Db(_) => "db_error",
            FindingsError::Internal(_) => "internal_error",
        }
    }
}

impl From<std::io::Error> for FindingsError {
    fn from(e: std::io::Error) -> Self {
        FindingsError::Io(e.to_string())
    }
}

impl From<rusqlite::Error> for FindingsError {
    fn from(e: rusqlite::Error) -> Self {
        FindingsError::Db(e.to_string())
    }
}

impl From<FindingsError> for AppError {
    fn from(err: FindingsError) -> Self {
        match err {
            FindingsError::InvalidInput(field) => AppError::InvalidInput(field.into()),
            FindingsError::ScanNotFound => AppError::ScanNotFound,
            FindingsError::FindingNotFound => AppError::FindingNotFound,
            FindingsError::NoRawOutput => AppError::FindingsNoRawOutput,
            FindingsError::RawOutputMissing => AppError::FindingsRawOutputMissing,
            FindingsError::ParseMalformed(s) => AppError::FindingsParseMalformed(s),
            FindingsError::AccountMismatch => AppError::FindingsAccountMismatch,
            FindingsError::Scanner(inner) => AppError::from(inner),
            FindingsError::Accounts(inner) => AppError::from(inner),
            FindingsError::Io(s) => AppError::Io(s),
            FindingsError::Db(s) => AppError::Db(s),
            FindingsError::Internal(tag) => AppError::Internal(format!("findings:{tag}")),
        }
    }
}
