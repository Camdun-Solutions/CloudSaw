// Shared, stable, enumerated error codes that cross the IPC boundary.
// Every fallible public function in CloudSaw returns `Result<T, AppError>`.
//
// Errors crossing IPC carry:
//   - `code`: a stable string (frontend can switch on it for localized copy)
//   - `message`: a short human-readable string (already redacted)
//
// Raw AWS SDK errors, raw stack traces, credentials, full ARNs, and full
// account IDs are NEVER serialized. See CLAUDE.md §4.2 and §4.4.

use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("io: {0}")]
    Io(String),

    #[error("path: {0}")]
    Path(String),

    #[error("db: {0}")]
    Db(String),

    #[error("migration: {0}")]
    Migration(String),

    #[error("config: {0}")]
    Config(String),

    // App-lock domain. Messages are intentionally generic for codes that
    // surface to the UI as the *result* of a security decision — see CLAUDE.md
    // §4.2 and Contract 02's "failed unlock must not leak whether a password
    // was close" rule.
    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("password rejected")]
    PasswordRejected,

    #[error("hash: {0}")]
    Hash(String),

    #[error("locked")]
    Locked,

    #[error("not configured")]
    NotConfigured,

    #[error("already configured")]
    AlreadyConfigured,

    #[error("rate limited: retry in {0}s")]
    RateLimited(u64),

    #[error("biometric: {0}")]
    Biometric(String),

    #[error("biometric unavailable")]
    BiometricUnavailable,

    #[error("identity verification: {0}")]
    IdentityVerification(String),

    #[error("identity verification unavailable")]
    IdentityVerificationUnavailable,

    // AWS auth domain (Contract 03). Messages are intentionally terse: the
    // frontend maps the `code` to a localized string, and these errors must
    // never carry credentials, full ARNs, or full account IDs (CLAUDE.md §4.2).
    #[error("aws config unreadable")]
    AwsConfigUnreadable,

    #[error("aws profile not found")]
    AwsProfileNotFound,

    #[error("aws timeout")]
    AwsTimeout,

    #[error("aws connectivity")]
    AwsConnectivity,

    #[error("aws sso expired")]
    AwsSsoExpired,

    #[error("aws permission denied: {0}")]
    AwsPermissionDenied(&'static str),

    // Multi-account domain (Contract 04). Account IDs and labels never appear
    // in these messages — the frontend maps the `code` to localized copy.
    #[error("account not found")]
    AccountNotFound,

    #[error("duplicate aws account id")]
    DuplicateAwsAccountId,

    #[error("duplicate label")]
    DuplicateLabel,

    #[error("aws account id mismatch")]
    AccountIdMismatch,

    // Terraform scanner-role provisioner (Contract 05). Like the AWS auth
    // domain, messages are intentionally terse — the frontend maps the
    // `code` to localized copy and these errors must never carry raw
    // Terraform stderr, ARNs, account IDs, or credential material.
    #[error("terraform not bundled")]
    TerraformNotBundled,

    #[error("terraform integrity failed")]
    TerraformIntegrityFailed,

    #[error("terraform init failed")]
    TerraformInitFailed,

    #[error("terraform plan failed")]
    TerraformPlanFailed,

    #[error("terraform apply failed")]
    TerraformApplyFailed,

    #[error("plan token expired")]
    TerraformPlanTokenExpired,

    #[error("plan token invalid")]
    TerraformPlanTokenInvalid,

    #[error("identity unresolvable")]
    TerraformIdentityUnresolvable,

    #[error("trust policy verification failed")]
    TerraformTrustVerificationFailed,

    #[error("internal: {0}")]
    Internal(String),
}

impl AppError {
    pub fn code(&self) -> &'static str {
        match self {
            AppError::Io(_) => "io_error",
            AppError::Path(_) => "path_error",
            AppError::Db(_) => "db_error",
            AppError::Migration(_) => "migration_error",
            AppError::Config(_) => "config_error",
            AppError::InvalidInput(_) => "invalid_input",
            AppError::PasswordRejected => "password_rejected",
            AppError::Hash(_) => "hash_error",
            AppError::Locked => "locked",
            AppError::NotConfigured => "not_configured",
            AppError::AlreadyConfigured => "already_configured",
            AppError::RateLimited(_) => "rate_limited",
            AppError::Biometric(_) => "biometric_error",
            AppError::BiometricUnavailable => "biometric_unavailable",
            AppError::IdentityVerification(_) => "identity_verification_error",
            AppError::IdentityVerificationUnavailable => "identity_verification_unavailable",
            AppError::AwsConfigUnreadable => "aws_config_unreadable",
            AppError::AwsProfileNotFound => "profile_not_found",
            AppError::AwsTimeout => "aws_timeout",
            AppError::AwsConnectivity => "aws_connectivity",
            AppError::AwsSsoExpired => "aws_sso_expired",
            AppError::AwsPermissionDenied(_) => "aws_permission_denied",
            AppError::AccountNotFound => "account_not_found",
            AppError::DuplicateAwsAccountId => "duplicate_aws_account_id",
            AppError::DuplicateLabel => "duplicate_label",
            AppError::AccountIdMismatch => "aws_account_id_mismatch",
            AppError::TerraformNotBundled => "terraform_not_bundled",
            AppError::TerraformIntegrityFailed => "terraform_integrity_failed",
            AppError::TerraformInitFailed => "terraform_init_failed",
            AppError::TerraformPlanFailed => "terraform_plan_failed",
            AppError::TerraformApplyFailed => "terraform_apply_failed",
            AppError::TerraformPlanTokenExpired => "terraform_plan_token_expired",
            AppError::TerraformPlanTokenInvalid => "terraform_plan_token_invalid",
            AppError::TerraformIdentityUnresolvable => "terraform_identity_unresolvable",
            AppError::TerraformTrustVerificationFailed => "terraform_trust_verification_failed",
            AppError::Internal(_) => "internal_error",
        }
    }
}

/// Serialized shape sent to the frontend.
#[derive(Serialize)]
pub struct IpcError {
    pub code: &'static str,
    pub message: String,
}

impl From<AppError> for IpcError {
    fn from(err: AppError) -> Self {
        IpcError {
            code: err.code(),
            message: err.to_string(),
        }
    }
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Cross-IPC shape: {code, message}. The code is the variant's stable
        // discriminator (see `code()`); the message is the Display string,
        // which thiserror already keeps redaction-friendly.
        IpcError {
            code: self.code(),
            message: self.to_string(),
        }
        .serialize(serializer)
    }
}

// Conversions from foreign errors into our typed enum. We collapse messages to
// strings on purpose — the source error type does not cross IPC.

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Io(e.to_string())
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        AppError::Db(e.to_string())
    }
}
