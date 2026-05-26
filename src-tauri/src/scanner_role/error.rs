// Typed errors for the scanner_role module. Each variant maps to a
// stable IPC error code via `code()`; the frontend `useIpcError.ts`
// hook maps those codes to localized strings.
//
// CLAUDE.md §4.2: no raw AWS error text, no stderr, no SDK strings
// reach the IPC boundary. The classifications here are derived purely
// from AWS error codes (e.g. "AccessDenied", "NoSuchEntity") and
// network failure shapes — never from the human-readable message.

use crate::auth::AuthError;
use crate::errors::AppError;
use crate::scanner::ScannerError;

#[derive(Debug, thiserror::Error)]
pub enum ScannerRoleError {
    /// `assume_role` was rejected by AWS with AccessDenied / similar.
    /// In practice this means one of: the role's trust policy doesn't
    /// trust the user's caller ARN, the `sts:ExternalId` condition
    /// doesn't match the value CloudSaw passed, or the role's resource
    /// policy explicitly denies the caller. The UI surfaces a
    /// remediation message that walks through both possibilities.
    #[error("scanner role assume denied — trust policy or external_id mismatch")]
    AssumeDenied,

    /// The role ARN parsed cleanly but `assume_role` returned
    /// NoSuchEntity. The user either typed the ARN wrong or hasn't
    /// actually created the role yet.
    #[error("scanner role not found at the supplied ARN")]
    NotFound,

    /// Any other AWS failure during the assume_role dry-run — timeout,
    /// network, expired SSO, etc. The UI message points to network /
    /// SSO recovery actions.
    #[error("scanner role assume failed")]
    AssumeFailed,

    /// The role's account portion of the ARN doesn't match the
    /// `aws_account_id` CloudSaw has configured for this account.
    /// Common cause: user pasted a role ARN from a different AWS
    /// account.
    #[error("scanner role ARN belongs to a different AWS account")]
    AccountIdMismatch,

    /// The caller-identity check on the user's configured profile
    /// returned an account different from the `aws_account_id`
    /// CloudSaw has on record. Means the user's `~/.aws/config` profile
    /// has changed since they first added the account. The UI directs
    /// them to fix the profile or re-add the account.
    #[error("profile caller identity does not match the configured account")]
    CallerAccountMismatch,

    /// The supplied role ARN doesn't parse as an IAM role ARN. The
    /// frontend already client-side validates against a regex; this
    /// branch is defense-in-depth for an IPC caller that bypassed the
    /// UI.
    #[error("supplied role ARN is malformed")]
    InvalidRoleArn,

    /// Caller-side validation failure (unknown policy variant tag,
    /// malformed account ID). String is a stable field name.
    #[error("invalid input: {0}")]
    InvalidInput(&'static str),

    /// Bubbled from `auth::get_caller_identity` — SSO expired, profile
    /// missing, permission denied on the caller's own identity. The
    /// inner error carries the stable `code()`.
    #[error("aws auth: {0}")]
    Auth(#[from] AuthError),

    /// Bubbled from `scanner::sts::assume_scanner_role`. The wrapper
    /// `From<ScannerError>` impl classifies the inner code into one of
    /// the variants above (AssumeDenied / NotFound / AssumeFailed).
    /// This variant is the catch-all for anything the classifier
    /// doesn't recognize.
    #[error("scanner sts: {0}")]
    ScannerSts(ScannerError),

    /// SQLite failure on the small persistence we do (external_id,
    /// scanner_role_arn, last_provisioning_error).
    #[error("db: {0}")]
    Db(String),

    /// Internal invariant violated (e.g. account row missing after
    /// `accounts::get` returned Ok — should not happen, but we
    /// classify defensively). The string is a stable source-code tag —
    /// never raw error text.
    #[error("internal: {0}")]
    Internal(&'static str),
}

impl ScannerRoleError {
    pub fn code(&self) -> &'static str {
        match self {
            ScannerRoleError::AssumeDenied => "scanner_role_assume_denied",
            ScannerRoleError::NotFound => "scanner_role_not_found",
            ScannerRoleError::AssumeFailed => "scanner_role_assume_failed",
            ScannerRoleError::AccountIdMismatch => "scanner_role_account_mismatch",
            ScannerRoleError::CallerAccountMismatch => "scanner_role_caller_account_mismatch",
            ScannerRoleError::InvalidRoleArn => "scanner_role_invalid_arn",
            ScannerRoleError::InvalidInput(_) => "invalid_input",
            ScannerRoleError::Auth(inner) => inner.code(),
            ScannerRoleError::ScannerSts(_) => "scanner_role_assume_failed",
            ScannerRoleError::Db(_) => "db_error",
            ScannerRoleError::Internal(_) => "internal_error",
        }
    }
}

impl From<rusqlite::Error> for ScannerRoleError {
    fn from(e: rusqlite::Error) -> Self {
        ScannerRoleError::Db(e.to_string())
    }
}

/// Classify a `ScannerError::AssumeRoleFailed(tag)` into our typed
/// variants. The scanner's classifier is the only source of truth for
/// what AWS returned — re-classifying with overlapping but distinct
/// labels here lets the UI surface different remediation copy for the
/// connect path (which has different recovery actions than a scan-time
/// failure).
impl From<ScannerError> for ScannerRoleError {
    fn from(err: ScannerError) -> Self {
        if let ScannerError::AssumeRoleFailed(tag) = &err {
            match *tag {
                "access_denied" => return ScannerRoleError::AssumeDenied,
                "expired" => {
                    // SSO session expired — surfaces through Auth so the
                    // UI shows the existing `aws.error.sso_expired` copy.
                    return ScannerRoleError::Auth(AuthError::SsoExpired);
                }
                _ => return ScannerRoleError::AssumeFailed,
            }
        }
        ScannerRoleError::ScannerSts(err)
    }
}

impl From<ScannerRoleError> for AppError {
    fn from(err: ScannerRoleError) -> Self {
        match err {
            ScannerRoleError::AssumeDenied => AppError::ScannerRoleAssumeDenied,
            ScannerRoleError::NotFound => AppError::ScannerRoleNotFound,
            ScannerRoleError::AssumeFailed => AppError::ScannerRoleAssumeFailed,
            ScannerRoleError::AccountIdMismatch => AppError::ScannerRoleAccountMismatch,
            ScannerRoleError::CallerAccountMismatch => AppError::ScannerRoleCallerAccountMismatch,
            ScannerRoleError::InvalidRoleArn => AppError::ScannerRoleInvalidArn,
            ScannerRoleError::InvalidInput(field) => AppError::InvalidInput(field.into()),
            ScannerRoleError::Auth(inner) => AppError::from(inner),
            ScannerRoleError::ScannerSts(_) => AppError::ScannerRoleAssumeFailed,
            ScannerRoleError::Db(s) => AppError::Db(s),
            ScannerRoleError::Internal(tag) => AppError::Internal(format!("scanner_role:{tag}")),
        }
    }
}
