// Public data types crossing the IPC boundary for the terraform module.
//
// Per CLAUDE.md §4.1, IPC payloads are plain serializable structs — no
// process handles, no file paths to in-app resources, no AWS SDK types.
// Every field below is a primitive or a deliberately enumerated tag.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Result of `detect_terraform`. The variants are mutually exclusive and the
/// UI switches on `status`. A missing or integrity-failed binary blocks all
/// downstream operations — `plan`/`apply` refuse to run.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum TerraformAvailability {
    /// Binary located and SHA-256 matched the build-pinned hash.
    Available {
        /// Hex SHA-256 of the bundled binary. Returned for diagnostic
        /// surfacing — UI displays the first 12 chars so the user can
        /// cross-reference with the release manifest.
        sha256: String,
        /// `terraform version` self-report. Optional because we run the call
        /// best-effort; the integrity check is the authoritative gate.
        version: Option<String>,
    },
    /// No binary bundled for this target triple. Production builds before
    /// Next Steps C2 wires up the per-target binary stay in this state.
    Missing,
    /// A binary was located but its SHA-256 did not match the build-pinned
    /// hash. This is a hard error — execution is refused until the user
    /// reinstalls a known-good CloudSaw build.
    IntegrityFailed,
}

/// Which AWS-managed policy to attach to the scanner role. Default is the
/// least-privilege `SecurityAudit`; `ReadOnlyAccess` is an opt-in surfaced
/// in the UI with an explicit warning per Contract 05 §Constraints.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyVariant {
    #[default]
    SecurityAudit,
    ReadOnlyAccess,
}

impl PolicyVariant {
    /// String form written into Terraform inputs and the database. Stable
    /// across versions — the migration uses these literal values.
    pub fn as_tf_str(self) -> &'static str {
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

    /// AWS-managed policy ARN. Used by the trust-verification step that
    /// re-reads the role after apply (Contract 05 acceptance criteria).
    pub fn managed_policy_arn(self) -> &'static str {
        match self {
            PolicyVariant::SecurityAudit => "arn:aws:iam::aws:policy/SecurityAudit",
            PolicyVariant::ReadOnlyAccess => "arn:aws:iam::aws:policy/ReadOnlyAccess",
        }
    }
}

/// Caller-side options passed to `plan`. Kept as a struct rather than a bare
/// arg so later contracts can add knobs (e.g. custom role-name suffix) without
/// breaking the IPC shape.
#[derive(Debug, Clone, Deserialize)]
pub struct PlanOptions {
    /// Explicit opt-in for the broader `ReadOnlyAccess` policy. Defaults to
    /// the least-privilege `SecurityAudit`.
    #[serde(default)]
    pub policy_variant: PolicyVariant,
}

impl Default for PlanOptions {
    fn default() -> Self {
        Self {
            policy_variant: PolicyVariant::SecurityAudit,
        }
    }
}

/// One line of the human-readable plan diff. `kind` drives the icon/colour
/// in the UI; `summary` is already redacted-by-construction (no full ARNs,
/// no account IDs). Account IDs are pre-masked to "****<last4>" by the
/// parser before they reach this struct.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PlanChange {
    pub kind: PlanChangeKind,
    /// Terraform resource address, e.g. `aws_iam_role.scanner`. Never carries
    /// the role's ARN.
    pub resource_address: String,
    /// Resource type, e.g. `aws_iam_role`. Used by the UI to group the diff.
    pub resource_type: String,
    /// One-line human summary. Inline diff lines (added/changed attribute
    /// names) live in `attributes`.
    pub summary: String,
    /// Optional pre-image / post-image attribute names. Values are not
    /// included — only the field names that would change, so we can never
    /// leak a credential through this surface.
    pub attributes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanChangeKind {
    Create,
    Update,
    Delete,
    Replace,
    NoOp,
    Read,
}

/// What `plan` returns to the UI. The `plan_token` ties a subsequent `apply`
/// to *this specific* plan — a fresh `plan` mints a new token and supersedes
/// any prior one for the same account (Contract 05 §Edge Cases).
#[derive(Debug, Clone, Serialize)]
pub struct PlanResult {
    pub plan_token: String,
    /// `true` when the plan would create no changes — the role already
    /// matches the desired state and `apply` is a safe no-op.
    pub no_changes: bool,
    /// Human-readable diff lines for the UI.
    pub changes: Vec<PlanChange>,
    /// The trust-policy principal CloudSaw will write. Surfaced so the user
    /// can confirm "yes, this is me" before clicking Apply. Returned in full
    /// because the user already owns this data (same reasoning as
    /// `CallerIdentity.arn` in Contract 03).
    pub planned_principal_arn: String,
    /// Policy variant CloudSaw will attach.
    pub policy_variant: PolicyVariant,
    /// When this plan was minted. UI displays the age to nudge re-planning
    /// if the user dawdles.
    pub created_at: DateTime<Utc>,
}

/// What `apply` returns. Always includes the final scanner-role ARN.
#[derive(Debug, Clone, Serialize)]
pub struct ApplyResult {
    pub role_arn: String,
    pub role_name: String,
    pub policy_variant: PolicyVariant,
    /// SHA-256 of the rendered trust policy JSON, useful for the QA
    /// acceptance check that confirms the principal matches the planned ARN.
    pub trust_policy_sha256: String,
}

/// Reported by `provisioning_status(account_id)`. Drives the per-account
/// "Provision scanner role" / "Re-plan" / "Show last error" UI affordances.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum ProvisioningStatus {
    /// No provisioning has been attempted for this account.
    NotProvisioned,
    /// `apply` succeeded; row carries the saved ARN and last-success time.
    Provisioned {
        role_arn: String,
        policy_variant: PolicyVariant,
        provisioned_at: DateTime<Utc>,
    },
    /// Last attempt failed; carries the stable error tag (no raw text).
    Failed {
        last_error_code: String,
        attempted_at: DateTime<Utc>,
    },
}
