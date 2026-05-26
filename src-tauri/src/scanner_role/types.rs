// Public data types crossing the IPC boundary for the scanner_role module.
//
// Phase 2 of the bundling/auth refactor: replaces the deleted
// `terraform::types` module. CloudSaw no longer provisions the scanner
// role itself — the user creates it (Console, Terraform, CloudFormation,
// or AWS CLI), and `scanner_role::connect()` validates + records it.
//
// `PolicyVariant`, `ApplyResult`, and `ProvisioningStatus` are moved
// verbatim from `terraform::types` so the SQLite schema (migration
// 0004) and the frontend types (`src/lib/ipc.ts`) keep their existing
// string forms — there is no migration cost to the existing `accounts`
// rows.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Which AWS-managed policy the user attached to the scanner role.
/// Default is the least-privilege `SecurityAudit`; `ReadOnlyAccess` is
/// an opt-in surfaced in the UI with an explicit warning per the
/// Contract 05 carry-over.
///
/// The string forms (`security_audit`, `read_only_access`) are the
/// stable storage representation in the `accounts.policy_variant`
/// column; migration 0004 stores those literals.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyVariant {
    #[default]
    SecurityAudit,
    ReadOnlyAccess,
}

impl PolicyVariant {
    /// Stable storage representation. Matches the literals migration 0004
    /// records into `accounts.policy_variant`.
    pub fn as_storage_str(self) -> &'static str {
        match self {
            PolicyVariant::SecurityAudit => "security_audit",
            PolicyVariant::ReadOnlyAccess => "read_only_access",
        }
    }

    pub fn from_storage(s: &str) -> Option<PolicyVariant> {
        match s {
            "security_audit" => Some(PolicyVariant::SecurityAudit),
            "read_only_access" => Some(PolicyVariant::ReadOnlyAccess),
            _ => None,
        }
    }

    /// AWS-managed policy ARN this variant maps to. Surfaced in the
    /// "Connect scanner role" recipes so users can copy-paste the ARN
    /// into their Console/Terraform/CFN/CLI invocation.
    pub fn managed_policy_arn(self) -> &'static str {
        match self {
            PolicyVariant::SecurityAudit => "arn:aws:iam::aws:policy/SecurityAudit",
            PolicyVariant::ReadOnlyAccess => "arn:aws:iam::aws:policy/ReadOnlyAccess",
        }
    }
}

/// What `scanner_role::connect()` returns on success.
///
/// Same shape as the old `terraform::ApplyResult` so the frontend type
/// in `src/lib/ipc.ts` can continue to consume it without any layout
/// change. `trust_policy_sha256` is `None` when the connecting profile
/// lacks `iam:GetRole` (the graceful-degradation path documented in the
/// Phase 2 plan): the connect itself still succeeds because the
/// AssumeRole dry-run is the authoritative validation, we just can't
/// re-read the trust policy to compute its SHA.
#[derive(Debug, Clone, Serialize)]
pub struct ConnectResult {
    pub role_arn: String,
    pub role_name: String,
    pub policy_variant: PolicyVariant,
    /// `None` when the connecting profile lacks `iam:GetRole` so the
    /// post-AssumeRole trust-policy verification was skipped. The
    /// connect is still recorded; the UI surfaces a soft note.
    pub trust_policy_sha256: Option<String>,
}

/// Reported by `scanner_role_status(account_id)`. Drives the per-account
/// "Connect scanner role" / "Re-connect" / "Show last error" UI
/// affordances. Identical shape to the deleted `terraform::ProvisioningStatus`
/// so the SQLite read path and frontend types stay aligned.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum ProvisioningStatus {
    /// No connect has been attempted for this account.
    NotProvisioned,
    /// `connect()` succeeded; row carries the saved ARN + last-success
    /// time.
    Provisioned {
        role_arn: String,
        policy_variant: PolicyVariant,
        provisioned_at: DateTime<Utc>,
    },
    /// Last connect attempt failed; carries the stable error tag (no
    /// raw text — see `error::ScannerRoleError::code()`).
    Failed {
        last_error_code: String,
        attempted_at: DateTime<Utc>,
    },
}

/// Surface returned to the UI's "How to create this role" recipes. The
/// frontend renders these values pre-substituted into the recipe text
/// so the user can copy-paste without editing. CloudSaw owns the
/// external_id because the user must use the *exact* string in their
/// trust-policy condition — typos there would cause `AssumeRole` to
/// reject the connect with no helpful diagnostic.
#[derive(Debug, Clone, Serialize)]
pub struct RoleRequirements {
    /// The principal CloudSaw will assume *from*. Pulled live from
    /// `sts:GetCallerIdentity` on the account's configured profile.
    pub trusted_principal_arn: String,
    /// 32-hex-char value the user MUST put in the trust policy's
    /// `sts:ExternalId` condition. Generated per-account and reused
    /// across re-connect attempts.
    pub external_id: String,
    /// Default policy variant the recipes pre-fill (user can switch in
    /// the form). The string form is stable across versions; the UI
    /// matches against `as_storage_str()`.
    pub default_policy_variant: PolicyVariant,
}
