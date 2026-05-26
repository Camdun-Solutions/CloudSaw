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

    // Scanner-role connect flow (Phase 2 — replaces the deleted Terraform
    // provisioner errors). Like the AWS auth domain, messages are
    // intentionally terse — the frontend maps the `code` to localized
    // copy and these errors must never carry raw AWS error text, ARNs,
    // account IDs, or credential material.
    #[error("scanner role assume denied")]
    ScannerRoleAssumeDenied,

    #[error("scanner role not found")]
    ScannerRoleNotFound,

    #[error("scanner role assume failed")]
    ScannerRoleAssumeFailed,

    #[error("scanner role ARN belongs to a different AWS account")]
    ScannerRoleAccountMismatch,

    #[error("profile caller identity does not match the configured account")]
    ScannerRoleCallerAccountMismatch,

    #[error("scanner role ARN is malformed")]
    ScannerRoleInvalidArn,

    // Scanner orchestrator (Contract 06). Stable codes only; raw scanner
    // stderr, ARNs, account IDs, or credential material never appear in any
    // of these messages.
    #[error("scanner not bundled")]
    ScannerNotBundled,

    #[error("scanner integrity failed")]
    ScannerIntegrityFailed,

    #[error("scanner role not provisioned")]
    ScannerRoleNotProvisioned,

    #[error("scan already running")]
    ScanAlreadyRunning,

    #[error("scan not found")]
    ScanNotFound,

    #[error("scanner assume role failed: {0}")]
    ScannerAssumeRoleFailed(&'static str),

    #[error("scanner spawn failed")]
    ScannerSpawnFailed,

    #[error("scanner process lost")]
    ScannerProcessLost,

    #[error("scanner process failed")]
    ScannerProcessFailed,

    #[error("scanner output missing")]
    ScannerOutputMissing,

    // Findings parser & store (Contract 07). Stable codes only; raw scanner
    // JSON, ARNs, account IDs, or credential material never appear in any
    // of these messages.
    #[error("finding not found")]
    FindingNotFound,

    #[error("findings: no raw output")]
    FindingsNoRawOutput,

    #[error("findings: raw output missing")]
    FindingsRawOutputMissing,

    #[error("findings: malformed scanner output: {0}")]
    FindingsParseMalformed(String),

    #[error("findings: account mismatch")]
    FindingsAccountMismatch,

    // Knowledge base & compliance mapping (Contract 08). The KB module only
    // reads bundled markdown and, when opted-in, public documentation —
    // these errors carry no credential material, ARNs, or account IDs.
    #[error("kb: remote refresh disabled")]
    KbRefreshDisabled,

    #[error("kb: remote refresh unreachable")]
    KbRefreshUnreachable,

    #[error("kb: remote refresh content invalid")]
    KbRefreshInvalidContent,

    #[error("kb: remote refresh already up to date")]
    KbRefreshUpToDate,

    // Scheduled & automated scans (Contract 10). Stable codes only; account
    // IDs / labels / scan output never appear in any of these messages.
    #[error("schedule not found")]
    ScheduleNotFound,

    // Event log, retention, hard delete & panic (Contract 11). Stable
    // codes only; no scan output, no credential material, no path content
    // ever appears in any of these messages.
    #[error("confirmation rejected")]
    ConfirmationRejected,

    // GitHub integration (Contract 12). Stable codes only; no PAT
    // material, no Authorization header, no raw API response body
    // appears in any of these messages.
    #[error("github: no token configured")]
    GithubNoToken,

    #[error("github: token invalid or expired")]
    GithubTokenInvalid,

    #[error("github: rate limited")]
    GithubRateLimited,

    #[error("github: network unreachable")]
    GithubNetwork,

    #[error("github: server error {0}")]
    GithubServerError(u16),

    #[error("github: no findings repo configured")]
    GithubNoFindingsRepo,

    #[error("github: ticket already exists")]
    GithubDuplicateTicket,

    // AI Suggestion Layer (Contract 13). Stable codes only; no provider
    // response body, no API key material, no finding identifier ever
    // appears in any of these messages.
    #[error("ai: no provider key")]
    AiNoProviderKey,

    #[error("ai: no provider configured")]
    AiNoProvider,

    #[error("ai: provider key invalid or expired")]
    AiKeyInvalid,

    #[error("ai: rate limited")]
    AiRateLimited,

    #[error("ai: network unreachable")]
    AiNetwork,

    #[error("ai: provider error {0}")]
    AiServerError(u16),

    // Report exporter (Contract 15). Stable codes only; no scan
    // content, no resource paths, and no filesystem path content
    // appears in any of these messages.
    #[error("report: output write failed")]
    ReportOutputWrite,

    #[error("report: pdf render failed: {0}")]
    ReportPdfRender(String),

    #[error("report: auto-export copy failed")]
    ReportAutoExportCopy,

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
            AppError::ScannerRoleAssumeDenied => "scanner_role_assume_denied",
            AppError::ScannerRoleNotFound => "scanner_role_not_found",
            AppError::ScannerRoleAssumeFailed => "scanner_role_assume_failed",
            AppError::ScannerRoleAccountMismatch => "scanner_role_account_mismatch",
            AppError::ScannerRoleCallerAccountMismatch => "scanner_role_caller_account_mismatch",
            AppError::ScannerRoleInvalidArn => "scanner_role_invalid_arn",
            AppError::ScannerNotBundled => "scanner_not_bundled",
            AppError::ScannerIntegrityFailed => "scanner_integrity_failed",
            AppError::ScannerRoleNotProvisioned => "scanner_role_not_provisioned",
            AppError::ScanAlreadyRunning => "scan_already_running",
            AppError::ScanNotFound => "scan_not_found",
            AppError::ScannerAssumeRoleFailed(_) => "scanner_assume_role_failed",
            AppError::ScannerSpawnFailed => "scanner_spawn_failed",
            AppError::ScannerProcessLost => "scanner_process_lost",
            AppError::ScannerProcessFailed => "scanner_process_failed",
            AppError::ScannerOutputMissing => "scanner_output_missing",
            AppError::FindingNotFound => "finding_not_found",
            AppError::FindingsNoRawOutput => "findings_no_raw_output",
            AppError::FindingsRawOutputMissing => "findings_raw_output_missing",
            AppError::FindingsParseMalformed(_) => "findings_parse_malformed",
            AppError::FindingsAccountMismatch => "findings_account_mismatch",
            AppError::KbRefreshDisabled => "kb_refresh_disabled",
            AppError::KbRefreshUnreachable => "kb_refresh_unreachable",
            AppError::KbRefreshInvalidContent => "kb_refresh_invalid_content",
            AppError::KbRefreshUpToDate => "kb_refresh_up_to_date",
            AppError::ScheduleNotFound => "schedule_not_found",
            AppError::ConfirmationRejected => "confirmation_rejected",
            AppError::GithubNoToken => "github_no_token",
            AppError::GithubTokenInvalid => "github_token_invalid",
            AppError::GithubRateLimited => "github_rate_limited",
            AppError::GithubNetwork => "github_network",
            AppError::GithubServerError(_) => "github_server_error",
            AppError::GithubNoFindingsRepo => "github_no_findings_repo",
            AppError::GithubDuplicateTicket => "github_duplicate_ticket",
            AppError::AiNoProviderKey => "ai_no_provider_key",
            AppError::AiNoProvider => "ai_no_provider",
            AppError::AiKeyInvalid => "ai_key_invalid",
            AppError::AiRateLimited => "ai_rate_limited",
            AppError::AiNetwork => "ai_network",
            AppError::AiServerError(_) => "ai_server_error",
            AppError::ReportOutputWrite => "report_output_write",
            AppError::ReportPdfRender(_) => "report_pdf_render",
            AppError::ReportAutoExportCopy => "report_auto_export_copy",
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
