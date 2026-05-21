// ScannerError — typed enum returned by every public `scanner::*` function.
//
// Like the terraform and auth modules, raw scanner stderr, full ARNs, or
// account IDs never appear in any variant. The categories below capture
// each failure mode the contract enumerates (CLAUDE.md §4.2, Contract 06
// §Edge Cases).
//
// `Internal(&'static str)` carries a stable source-code constant only,
// never a value derived from a child process or SDK response.

use crate::accounts::AccountsError;
use crate::auth::AuthError;
use crate::errors::AppError;

#[derive(Debug, thiserror::Error)]
pub enum ScannerError {
    /// No ScoutSuite binary bundled for the current target triple. Surfaces
    /// from `detect_binary` and from `run_scan` (which refuses to proceed).
    #[error("scanner not bundled")]
    NotBundled,

    /// A ScoutSuite binary was located but its SHA-256 did not match the
    /// build-pinned hash. CLAUDE.md §4.5: "Bundled binary SHA-256 is verified
    /// against the build-pinned hash before every execution."
    #[error("scanner integrity failed")]
    IntegrityFailed,

    /// The selected account exists but has no scanner role provisioned yet.
    /// Returned before any AWS call so the UI can route the user to the
    /// provisioning flow (Contract 05).
    #[error("scanner role not provisioned")]
    RoleNotProvisioned,

    /// A scan is already running for this account. Per Contract 06
    /// §Constraints, only one scan per account may run at a time.
    #[error("scan already running")]
    AlreadyRunning,

    /// `scan_status(scan_id)` was called for a scan we have no record of.
    #[error("scan not found")]
    ScanNotFound,

    /// `sts:AssumeRole` failed. The inner static string is a stable tag
    /// (e.g. "access_denied", "expired", "timeout") — never raw SDK text.
    #[error("assume role failed: {0}")]
    AssumeRoleFailed(&'static str),

    /// `Command::spawn` itself failed (binary unreadable, missing exec bit,
    /// out of process handles). Surfaced after the SHA-256 gate so this is
    /// always a runtime OS problem, never a tampered-binary signal.
    #[error("scanner spawn failed")]
    SpawnFailed,

    /// The orchestrator was tracking a child process that disappeared
    /// between status polls (machine sleep, OS-level kill). Contract 06
    /// §Edge Cases requires marking the scan `Failed` with this reason.
    #[error("scanner process lost")]
    ProcessLost,

    /// The ScoutSuite child exited with a non-zero status that doesn't map
    /// to the "complete with warnings" branch.
    #[error("scanner process failed")]
    ProcessFailed,

    /// Scanner exited cleanly but produced no `raw-scout.json`. Treated as
    /// a hard failure so Contract 07 never tries to parse a nonexistent file.
    #[error("scanner output missing")]
    OutputMissing,

    /// Caller-side validation failure (malformed account ID, empty scan_id).
    /// Inner string is a stable field name.
    #[error("invalid input: {0}")]
    InvalidInput(&'static str),

    /// Bubbled from `auth::*` — SSO expired, profile missing, etc.
    #[error("aws auth: {0}")]
    Auth(#[from] AuthError),

    /// Bubbled from `accounts::*` — typically `AccountNotFound`.
    #[error("accounts: {0}")]
    Accounts(#[from] AccountsError),

    /// Filesystem failure inside the per-scan output directory.
    #[error("scan io: {0}")]
    ScanIo(String),

    /// SQLite failure on the scans table.
    #[error("db: {0}")]
    Db(String),

    /// Internal invariant violated. The string is a stable source-code tag.
    #[error("internal: {0}")]
    Internal(&'static str),
}

impl ScannerError {
    pub fn code(&self) -> &'static str {
        match self {
            ScannerError::NotBundled => "scanner_not_bundled",
            ScannerError::IntegrityFailed => "scanner_integrity_failed",
            ScannerError::RoleNotProvisioned => "scanner_role_not_provisioned",
            ScannerError::AlreadyRunning => "scan_already_running",
            ScannerError::ScanNotFound => "scan_not_found",
            ScannerError::AssumeRoleFailed(_) => "scanner_assume_role_failed",
            ScannerError::SpawnFailed => "scanner_spawn_failed",
            ScannerError::ProcessLost => "scanner_process_lost",
            ScannerError::ProcessFailed => "scanner_process_failed",
            ScannerError::OutputMissing => "scanner_output_missing",
            ScannerError::InvalidInput(_) => "invalid_input",
            ScannerError::Auth(inner) => inner.code(),
            ScannerError::Accounts(inner) => inner.code(),
            ScannerError::ScanIo(_) => "io_error",
            ScannerError::Db(_) => "db_error",
            ScannerError::Internal(_) => "internal_error",
        }
    }
}

impl From<std::io::Error> for ScannerError {
    fn from(e: std::io::Error) -> Self {
        ScannerError::ScanIo(e.to_string())
    }
}

impl From<rusqlite::Error> for ScannerError {
    fn from(e: rusqlite::Error) -> Self {
        ScannerError::Db(e.to_string())
    }
}

impl From<ScannerError> for AppError {
    fn from(err: ScannerError) -> Self {
        match err {
            ScannerError::NotBundled => AppError::ScannerNotBundled,
            ScannerError::IntegrityFailed => AppError::ScannerIntegrityFailed,
            ScannerError::RoleNotProvisioned => AppError::ScannerRoleNotProvisioned,
            ScannerError::AlreadyRunning => AppError::ScanAlreadyRunning,
            ScannerError::ScanNotFound => AppError::ScanNotFound,
            ScannerError::AssumeRoleFailed(tag) => AppError::ScannerAssumeRoleFailed(tag),
            ScannerError::SpawnFailed => AppError::ScannerSpawnFailed,
            ScannerError::ProcessLost => AppError::ScannerProcessLost,
            ScannerError::ProcessFailed => AppError::ScannerProcessFailed,
            ScannerError::OutputMissing => AppError::ScannerOutputMissing,
            ScannerError::InvalidInput(field) => AppError::InvalidInput(field.into()),
            ScannerError::Auth(inner) => AppError::from(inner),
            ScannerError::Accounts(inner) => AppError::from(inner),
            ScannerError::ScanIo(s) => AppError::Io(s),
            ScannerError::Db(s) => AppError::Db(s),
            ScannerError::Internal(tag) => AppError::Internal(format!("scanner:{tag}")),
        }
    }
}
