// TerraformError — typed enum returned by every public `terraform::*`
// function. Each variant maps to a stable, enumerated IPC error code via
// `code()` and folds into `AppError` for serialization.
//
// Like `AuthError` and `AccountsError`, no raw Terraform stderr, full ARN, or
// account ID is ever serialized through here. The categories below capture
// each failure mode the contract enumerates (CLAUDE.md §4.2, Contract 05
// §Edge Cases).
//
// Crucially, the `Internal(&'static str)` tag carries a stable, source-code
// constant — never a value derived from a Terraform invocation or an SDK
// response.

use crate::auth::AuthError;
use crate::errors::AppError;

#[derive(Debug, thiserror::Error)]
pub enum TerraformError {
    /// No Terraform binary was bundled for the current target triple. Surfaces
    /// from `detect_terraform` and from `plan`/`apply` (which refuse to
    /// proceed without a binary).
    #[error("terraform not bundled")]
    NotBundled,

    /// A Terraform binary was located but its SHA-256 did not match the
    /// build-pinned hash. CLAUDE.md §4.5: "Bundled binary SHA-256 is verified
    /// against the build-pinned hash before every execution." Any byte change
    /// halts execution.
    #[error("terraform integrity failed")]
    IntegrityFailed,

    /// `terraform init` failed. Most likely the AWS provider download failed
    /// (offline / corp proxy) or the working directory is unwritable.
    #[error("terraform init failed")]
    InitFailed,

    /// `terraform plan` failed before producing a plan file.
    #[error("terraform plan failed")]
    PlanFailed,

    /// `terraform apply` failed; partial state may exist and a subsequent
    /// apply will resume / converge.
    #[error("terraform apply failed")]
    ApplyFailed,

    /// `apply(plan_token)` was called with a token CloudSaw has no record of —
    /// either it was never minted, was for a different account, or the
    /// in-memory store was cleared (e.g. by app restart).
    #[error("plan token invalid")]
    PlanTokenInvalid,

    /// `apply(plan_token)` was called with a token that has been superseded
    /// by a newer `plan()` against the same account. Stale applies are
    /// rejected so the user can't approve diff A and apply plan B.
    #[error("plan token expired")]
    PlanTokenExpired,

    /// `sts:GetCallerIdentity` returned, but the resulting ARN could not be
    /// resolved to a valid trust-policy principal (e.g. unrecognized ARN
    /// shape). Refuses to fall back to a wildcard — Contract 05 §Constraints.
    #[error("identity unresolvable")]
    IdentityUnresolvable,

    /// After apply, CloudSaw re-reads the role's trust policy and confirms
    /// the principal exactly matches the planned ARN. A mismatch (or absence)
    /// surfaces here rather than silently passing.
    #[error("trust policy verification failed")]
    TrustVerificationFailed,

    /// Caller-side validation failure (malformed account ID, unknown policy
    /// variant tag). String is a stable field name.
    #[error("invalid input: {0}")]
    InvalidInput(&'static str),

    /// Bubbled from `auth::get_caller_identity` — SSO expired, profile
    /// missing, permission denied, etc. The inner error carries the stable
    /// `code()`.
    #[error("aws auth: {0}")]
    Auth(#[from] AuthError),

    /// Filesystem failure inside the per-account working directory. The
    /// inner string is the OS error message; that is acceptable here because
    /// we constructed the path and it does not contain secrets.
    #[error("workdir io: {0}")]
    WorkdirIo(String),

    /// SQLite failure on the small persistence we do (external_id,
    /// scanner_role_arn, last_provisioning_error).
    #[error("db: {0}")]
    Db(String),

    /// Internal invariant violated (e.g. plan-file JSON parse failure on a
    /// shape Terraform documented). The string is a stable source-code tag —
    /// never raw error text.
    #[error("internal: {0}")]
    Internal(&'static str),
}

impl TerraformError {
    pub fn code(&self) -> &'static str {
        match self {
            TerraformError::NotBundled => "terraform_not_bundled",
            TerraformError::IntegrityFailed => "terraform_integrity_failed",
            TerraformError::InitFailed => "terraform_init_failed",
            TerraformError::PlanFailed => "terraform_plan_failed",
            TerraformError::ApplyFailed => "terraform_apply_failed",
            TerraformError::PlanTokenInvalid => "terraform_plan_token_invalid",
            TerraformError::PlanTokenExpired => "terraform_plan_token_expired",
            TerraformError::IdentityUnresolvable => "terraform_identity_unresolvable",
            TerraformError::TrustVerificationFailed => "terraform_trust_verification_failed",
            TerraformError::InvalidInput(_) => "invalid_input",
            TerraformError::Auth(inner) => inner.code(),
            TerraformError::WorkdirIo(_) => "io_error",
            TerraformError::Db(_) => "db_error",
            TerraformError::Internal(_) => "internal_error",
        }
    }
}

impl From<std::io::Error> for TerraformError {
    fn from(e: std::io::Error) -> Self {
        TerraformError::WorkdirIo(e.to_string())
    }
}

impl From<rusqlite::Error> for TerraformError {
    fn from(e: rusqlite::Error) -> Self {
        TerraformError::Db(e.to_string())
    }
}

impl From<TerraformError> for AppError {
    fn from(err: TerraformError) -> Self {
        match err {
            TerraformError::NotBundled => AppError::TerraformNotBundled,
            TerraformError::IntegrityFailed => AppError::TerraformIntegrityFailed,
            TerraformError::InitFailed => AppError::TerraformInitFailed,
            TerraformError::PlanFailed => AppError::TerraformPlanFailed,
            TerraformError::ApplyFailed => AppError::TerraformApplyFailed,
            TerraformError::PlanTokenInvalid => AppError::TerraformPlanTokenInvalid,
            TerraformError::PlanTokenExpired => AppError::TerraformPlanTokenExpired,
            TerraformError::IdentityUnresolvable => AppError::TerraformIdentityUnresolvable,
            TerraformError::TrustVerificationFailed => AppError::TerraformTrustVerificationFailed,
            TerraformError::InvalidInput(field) => AppError::InvalidInput(field.into()),
            TerraformError::Auth(inner) => AppError::from(inner),
            TerraformError::WorkdirIo(s) => AppError::Io(s),
            TerraformError::Db(s) => AppError::Db(s),
            TerraformError::Internal(tag) => AppError::Internal(format!("terraform:{tag}")),
        }
    }
}
