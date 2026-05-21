// Terraform scanner-role provisioner — Contract 05.
//
// CloudSaw bundles a Terraform binary plus a tiny IAM-role module
// (`src-tauri/tf-modules/scanner-role/`). This module orchestrates:
//
//     detect_terraform()                                 -> TerraformAvailability
//     plan(account_id, options)                          -> PlanResult
//     apply(account_id, plan_token)                      -> ApplyResult
//     provisioning_status(account_id)                    -> ProvisioningStatus
//
// Each fallible function returns `Result<T, TerraformError>`.
//
// What this module deliberately does NOT do (CLAUDE.md §5 + Contract 05
// §Constraints):
//   - Invoke Terraform through a shell. argv arrays only, absolute path only,
//     SHA-256 verified before every spawn.
//   - Accept a trust-policy principal from the frontend. The trust policy
//     principal is the result of `sts:GetCallerIdentity`, unwrapped from any
//     assumed-role/SSO session by `identity::underlying_principal_arn`.
//   - Expose `terraform destroy`. Removal is out-of-band by design.
//   - Cache STS credentials. `auth::get_caller_identity` is called once per
//     plan; the SDK provider chain handles refresh.
//   - Store Terraform state outside the per-account workdir.

pub mod binary;
pub mod error;
pub mod identity;
pub mod plans;
pub mod runner;
pub mod storage;
pub mod types;
pub mod workdir;

pub use error::TerraformError;
pub use types::{
    ApplyResult, PlanChange, PlanChangeKind, PlanOptions, PlanResult, PolicyVariant,
    ProvisioningStatus, TerraformAvailability,
};

use chrono::DateTime;

/// Detect whether a bundled Terraform binary is present AND passes its
/// SHA-256 integrity check. Pure local-state — no AWS calls, no account
/// scope. The frontend gates the entire provisioning UI on this.
pub fn detect_terraform() -> TerraformAvailability {
    binary::availability()
}

/// Generate (and idempotently re-generate) a plan for the given account.
/// Each successful call mints a fresh `plan_token` and supersedes any prior
/// plan for the same account.
pub async fn plan(
    aws_account_id: &str,
    options: PlanOptions,
) -> Result<PlanResult, TerraformError> {
    runner::plan(aws_account_id, options).await
}

/// Apply a previously confirmed plan, identified by `plan_token`. On success
/// records the role ARN and policy variant into the accounts table.
pub async fn apply(
    aws_account_id: &str,
    plan_token: &str,
) -> Result<ApplyResult, TerraformError> {
    runner::apply(aws_account_id, plan_token).await
}

/// Report the provisioning state for `aws_account_id`. Drives the per-account
/// "Provision scanner role" / "Re-plan" / "Show last error" UI.
pub fn provisioning_status(aws_account_id: &str) -> Result<ProvisioningStatus, TerraformError> {
    let row = storage::provisioning_row(aws_account_id)?;
    if row.role_provisioned {
        let role_arn = row
            .scanner_role_arn
            .ok_or(TerraformError::Internal("provisioned_without_arn"))?;
        let variant = row
            .policy_variant
            .as_deref()
            .and_then(PolicyVariant::from_storage)
            .unwrap_or(PolicyVariant::SecurityAudit);
        let provisioned_at = parse_ts_required(
            row.role_provisioned_at
                .as_deref()
                .ok_or(TerraformError::Internal("provisioned_without_timestamp"))?,
        )?;
        return Ok(ProvisioningStatus::Provisioned {
            role_arn,
            policy_variant: variant,
            provisioned_at,
        });
    }
    if let Some(code) = row.last_provisioning_error {
        let attempted_at = parse_ts_required(&row.updated_at)?;
        return Ok(ProvisioningStatus::Failed {
            last_error_code: code,
            attempted_at,
        });
    }
    Ok(ProvisioningStatus::NotProvisioned)
}

fn parse_ts_required(s: &str) -> Result<chrono::DateTime<chrono::Utc>, TerraformError> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .map_err(|_| TerraformError::Internal("bad_timestamp"))
}
