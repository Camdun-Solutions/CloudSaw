// AuthError — typed enum returned by every public `auth::*` function.
// Each variant maps to a stable, enumerated IPC error code via `code()`.
// Raw AWS SDK errors never propagate; they are categorized into these
// variants inside `sts.rs` so the UI sees a stable surface.
//
// `From<AuthError> for AppError` lets the IPC layer reuse the existing
// `Result<T, AppError>` serialization while preserving the auth-specific
// error code.

use crate::errors::AppError;

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    /// `~/.aws/config` (or `AWS_CONFIG_FILE`) is unreadable or malformed.
    #[error("aws config unreadable")]
    ConfigUnreadable,

    /// Profile not present in `~/.aws/config`.
    #[error("profile not found")]
    ProfileNotFound,

    /// Profile name contained disallowed characters; never passed to the SDK.
    #[error("invalid profile name")]
    InvalidProfileName,

    /// STS call did not return inside the bounded timeout.
    #[error("aws timeout")]
    Timeout,

    /// Network unreachable / TLS handshake failed / DNS failed.
    #[error("aws connectivity")]
    Connectivity,

    /// SSO session is expired or has never been refreshed.
    #[error("aws sso expired")]
    SsoExpired,

    /// STS returned AccessDenied / Forbidden for the failing API.
    /// The string is the failing API name (e.g. "GetCallerIdentity"),
    /// never an ARN or account ID.
    #[error("aws permission denied: {0}")]
    PermissionDenied(&'static str),

    /// Catch-all for unexpected SDK or runtime failures. The message is a
    /// short tag (`sdk_construction`, `sdk_response`, …) — never a raw error
    /// message, never credential material.
    #[error("internal: {0}")]
    Internal(&'static str),

    /// PR #66 — `auth_create_profile` rejected a duplicate. The
    /// frontend pre-checks the loaded profile list before submit, so
    /// this only fires on a race or when the name only collides in
    /// `~/.aws/credentials` but not `~/.aws/config`.
    #[error("aws profile already exists")]
    DuplicateProfileName,

    /// PR #66 — writing the new section to `~/.aws/credentials` or
    /// `~/.aws/config` failed. The tag identifies which step
    /// (`open`, `credentials_write`, `config_write`) without leaking
    /// the underlying I/O error message.
    #[error("aws config write failed: {0}")]
    ConfigWriteFailed(&'static str),
}

impl AuthError {
    pub fn code(&self) -> &'static str {
        match self {
            AuthError::ConfigUnreadable => "aws_config_unreadable",
            AuthError::ProfileNotFound => "profile_not_found",
            AuthError::InvalidProfileName => "invalid_input",
            AuthError::Timeout => "aws_timeout",
            AuthError::Connectivity => "aws_connectivity",
            AuthError::SsoExpired => "aws_sso_expired",
            AuthError::PermissionDenied(_) => "aws_permission_denied",
            AuthError::Internal(_) => "internal_error",
            AuthError::DuplicateProfileName => "aws_profile_already_exists",
            AuthError::ConfigWriteFailed(_) => "aws_config_write_failed",
        }
    }
}

impl From<AuthError> for AppError {
    fn from(err: AuthError) -> Self {
        match err {
            AuthError::ConfigUnreadable => AppError::AwsConfigUnreadable,
            AuthError::ProfileNotFound => AppError::AwsProfileNotFound,
            AuthError::InvalidProfileName => AppError::InvalidInput("profile name".into()),
            AuthError::Timeout => AppError::AwsTimeout,
            AuthError::Connectivity => AppError::AwsConnectivity,
            AuthError::SsoExpired => AppError::AwsSsoExpired,
            AuthError::PermissionDenied(api) => AppError::AwsPermissionDenied(api),
            AuthError::Internal(tag) => AppError::Internal(format!("auth:{tag}")),
            AuthError::DuplicateProfileName => AppError::AwsProfileAlreadyExists,
            AuthError::ConfigWriteFailed(tag) => AppError::AwsConfigWriteFailed(tag),
        }
    }
}
