// The Phase 2 "connect scanner role" flow.
//
// Replaces the deleted `terraform::{plan, apply}` pair with a single
// validation step. The user creates `CloudSawScannerRole` themselves
// (Console, Terraform, CloudFormation, AWS CLI — the recipe of their
// choice, all rendered with values pre-substituted by the frontend);
// `connect()` takes the resulting ARN, validates it via dry-run
// `sts:AssumeRole`, and records it against the existing `accounts`
// row.
//
// The validation chain is layered so the UI can surface the most
// specific actionable error for whatever the user got wrong:
//
//   1. Parse the role ARN (catches typos before any AWS call).
//   2. Verify the role's account portion matches the configured
//      `aws_account_id` (catches "pasted a role from a different
//      account").
//   3. `sts:GetCallerIdentity` on the user's profile (catches "the
//      profile in ~/.aws/config has changed since this account was
//      added").
//   4. Read or generate the external_id from SQLite (this is the
//      string the user MUST have put in their trust policy's
//      `sts:ExternalId` condition).
//   5. Dry-run `sts:AssumeRole` against the supplied ARN with our
//      external_id (the authoritative check — passes only if the
//      role exists, trusts the user's caller, and the external_id
//      condition matches).
//   6. Persist via `storage::record_provisioned()`.
//
// Each step's failure mode maps to a distinct `ScannerRoleError`
// variant so the frontend can render specific copy. CLAUDE.md §4.2:
// no raw AWS strings reach the IPC boundary.

use crate::accounts;
use crate::auth;
use crate::scanner::sts as scanner_sts;

use super::error::ScannerRoleError;
use super::storage;
use super::types::{ConnectResult, PolicyVariant, RoleRequirements};

/// Role-session name used by the connect-time dry-run AssumeRole.
/// CloudTrail surfaces this as `<role>/cloudsaw-connect` — distinct
/// from the per-scan session names so audit logs can tell "user
/// connected the role" apart from "scanner used the role". AWS
/// constraint: 2-64 chars, [\w+=,.@-]+.
const CONNECT_SESSION_NAME: &str = "cloudsaw-connect";

/// Build the recipe-display values for the "Connect scanner role"
/// form. Called when the wizard renders step 4 (and when the user
/// re-opens the form to re-connect). Returns the live caller ARN +
/// per-account external_id pre-formatted for direct substitution into
/// the four recipe blocks.
pub async fn requirements(aws_account_id: &str) -> Result<RoleRequirements, ScannerRoleError> {
    let account = accounts::get_account(aws_account_id)
        .map_err(|_| ScannerRoleError::InvalidInput("aws_account_id"))?;
    let caller = auth::get_caller_identity(&account.profile_name).await?;
    if caller.account_id != aws_account_id {
        return Err(ScannerRoleError::CallerAccountMismatch);
    }
    let external_id = storage::ensure_external_id(aws_account_id)?;
    Ok(RoleRequirements {
        trusted_principal_arn: caller.arn,
        external_id,
        default_policy_variant: PolicyVariant::SecurityAudit,
    })
}

/// Connect an externally-provisioned scanner role to CloudSaw.
///
/// On success, the account row's `scanner_role_arn`, `policy_variant`,
/// and `role_provisioned` columns are updated to match the supplied
/// values; subsequent scans assume the role via the existing
/// `scanner::sts::assume_scanner_role()` path with the same
/// external_id `ensure_external_id()` returned here.
///
/// On any failure the error is recorded via `storage::record_failure()`
/// so the UI can surface "last attempt failed with: …" on a return
/// visit. The error itself is also returned so the immediate caller
/// can render a specific message.
pub async fn connect(
    aws_account_id: &str,
    role_arn: &str,
    policy_variant: PolicyVariant,
) -> Result<ConnectResult, ScannerRoleError> {
    match connect_inner(aws_account_id, role_arn, policy_variant).await {
        Ok(result) => Ok(result),
        Err(err) => {
            // Best-effort failure record. If the storage write itself
            // fails (rare — same DB the rest of the app uses) we still
            // return the original validation error, since that's what
            // the user needs to act on.
            let _ = storage::record_failure(aws_account_id, err.code());
            Err(err)
        }
    }
}

async fn connect_inner(
    aws_account_id: &str,
    role_arn: &str,
    policy_variant: PolicyVariant,
) -> Result<ConnectResult, ScannerRoleError> {
    // 1. Parse the role ARN. We accept the standard shape only —
    //    `arn:aws:iam::<account>:role/<name>` (no path prefix support
    //    yet; can extend later if a user reports needing it).
    let parsed = parse_iam_role_arn(role_arn).ok_or(ScannerRoleError::InvalidRoleArn)?;

    // 2. Match the role's account against the configured account ID
    //    before any AWS call. Catches "wrong account" without burning
    //    an STS API budget.
    if parsed.account_id != aws_account_id {
        return Err(ScannerRoleError::AccountIdMismatch);
    }

    // 3. Resolve the account record + live caller identity.
    let account = accounts::get_account(aws_account_id)
        .map_err(|_| ScannerRoleError::InvalidInput("aws_account_id"))?;
    let caller = auth::get_caller_identity(&account.profile_name).await?;
    if caller.account_id != aws_account_id {
        return Err(ScannerRoleError::CallerAccountMismatch);
    }

    // 4. Load or generate the external_id we displayed in the recipes.
    let external_id = storage::ensure_external_id(aws_account_id)?;

    // 5. Dry-run AssumeRole. The scanner's existing classifier converts
    //    SDK errors into `ScannerError::AssumeRoleFailed(<tag>)`, and
    //    our `From<ScannerError>` impl narrows those tags into the
    //    specific `ScannerRoleError` variants the UI surfaces. Any
    //    non-classified SDK error becomes `AssumeFailed`.
    let _temp_creds = scanner_sts::assume_scanner_role(
        &account.profile_name,
        role_arn,
        &external_id,
        CONNECT_SESSION_NAME,
    )
    .await
    .map_err(|e| classify_assume_error(&e))?;

    // 6. Persist. The trust-policy SHA is left `None` for now — the
    //    optional `iam:GetRole`-based re-verification path is a
    //    follow-up; AssumeRole succeeding is the authoritative
    //    validation for this connect.
    storage::record_provisioned(aws_account_id, role_arn, policy_variant)?;

    Ok(ConnectResult {
        role_arn: role_arn.to_string(),
        role_name: parsed.role_name,
        policy_variant,
        trust_policy_sha256: None,
    })
}

/// Mapping wrapper around the scanner's STS classifier. The scanner
/// already collapses AWS error codes into stable tags
/// (`"access_denied"`, `"expired"`, etc.); we re-narrow those into our
/// connect-flow variants so the UI can render copy that's specific to
/// the connect path (which has different remediation actions than a
/// scan-time AssumeRole failure).
fn classify_assume_error(err: &crate::scanner::ScannerError) -> ScannerRoleError {
    // The owned-value `From<ScannerError>` impl is the canonical
    // conversion; we have a borrowed ref here, so build a fresh tagged
    // variant the From impl can consume. Avoids cloning the scanner
    // error type which doesn't impl Clone.
    use crate::scanner::ScannerError as S;
    match err {
        S::AssumeRoleFailed(tag) => match *tag {
            "access_denied" => ScannerRoleError::AssumeDenied,
            "expired" => ScannerRoleError::Auth(crate::auth::AuthError::SsoExpired),
            _ => ScannerRoleError::AssumeFailed,
        },
        // Anything that isn't an AssumeRoleFailed variant is unexpected
        // in the connect path — the scanner's STS helper only ever
        // returns AssumeRoleFailed(<tag>) for this call site. Classify
        // generically.
        _ => ScannerRoleError::AssumeFailed,
    }
}

struct ParsedIamRoleArn {
    account_id: String,
    role_name: String,
}

/// Parse `arn:aws:iam::<12-digit-account>:role/<role-name>` without
/// pulling in an ARN crate. Conservative: rejects path-prefixed roles
/// (e.g. `:role/my/path/MyRole`) since the scanner-role recipes don't
/// produce those. Returning `None` lets the caller surface
/// `InvalidRoleArn` rather than letting a malformed ARN go to AWS.
fn parse_iam_role_arn(arn: &str) -> Option<ParsedIamRoleArn> {
    let parts: Vec<&str> = arn.split(':').collect();
    // arn:aws:iam::<account>:<resource>  →  6 parts
    if parts.len() != 6 {
        return None;
    }
    if parts[0] != "arn" || parts[1] != "aws" || parts[2] != "iam" {
        return None;
    }
    if !parts[3].is_empty() {
        // IAM ARNs have an empty region segment.
        return None;
    }
    if parts[4].len() != 12 || !parts[4].chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let resource = parts[5];
    let role_name = resource.strip_prefix("role/")?;
    if role_name.is_empty() || role_name.contains('/') {
        // Reject path-prefixed roles for now.
        return None;
    }
    Some(ParsedIamRoleArn {
        account_id: parts[4].to_string(),
        role_name: role_name.to_string(),
    })
}

/// Translate the storage row into the user-facing
/// `ProvisioningStatus` enum, applying the same defaults the
/// frontend already understands (NotProvisioned when role_provisioned
/// is 0 and no prior failure has been recorded).
pub fn status(aws_account_id: &str) -> Result<super::types::ProvisioningStatus, ScannerRoleError> {
    use super::types::ProvisioningStatus;
    let row = storage::provisioning_row(aws_account_id)?;

    if row.role_provisioned {
        let role_arn = row
            .scanner_role_arn
            .ok_or(ScannerRoleError::Internal("missing_role_arn_after_success"))?;
        let policy_variant = row
            .policy_variant
            .as_deref()
            .and_then(PolicyVariant::from_storage)
            .ok_or(ScannerRoleError::Internal(
                "missing_policy_variant_after_success",
            ))?;
        let provisioned_at_str = row.role_provisioned_at.ok_or(ScannerRoleError::Internal(
            "missing_provisioned_at_after_success",
        ))?;
        let provisioned_at = chrono::DateTime::parse_from_rfc3339(&provisioned_at_str)
            .map_err(|_| ScannerRoleError::Internal("malformed_provisioned_at"))?
            .with_timezone(&chrono::Utc);
        return Ok(ProvisioningStatus::Provisioned {
            role_arn,
            policy_variant,
            provisioned_at,
        });
    }

    if let Some(code) = row.last_provisioning_error {
        let attempted_at = chrono::DateTime::parse_from_rfc3339(&row.updated_at)
            .map_err(|_| ScannerRoleError::Internal("malformed_updated_at"))?
            .with_timezone(&chrono::Utc);
        return Ok(ProvisioningStatus::Failed {
            last_error_code: code,
            attempted_at,
        });
    }

    Ok(ProvisioningStatus::NotProvisioned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_canonical_role_arn() {
        let p = parse_iam_role_arn("arn:aws:iam::123456789012:role/CloudSawScannerRole").unwrap();
        assert_eq!(p.account_id, "123456789012");
        assert_eq!(p.role_name, "CloudSawScannerRole");
    }

    #[test]
    fn rejects_non_iam_arn() {
        assert!(parse_iam_role_arn("arn:aws:s3:::bucket/key").is_none());
        assert!(parse_iam_role_arn("arn:aws:iam::123456789012:user/jane").is_none());
    }

    #[test]
    fn rejects_path_prefixed_role() {
        assert!(
            parse_iam_role_arn("arn:aws:iam::123456789012:role/team/CloudSawScannerRole").is_none()
        );
    }

    #[test]
    fn rejects_bad_account_id() {
        assert!(parse_iam_role_arn("arn:aws:iam::12:role/X").is_none());
        assert!(parse_iam_role_arn("arn:aws:iam::not-numeric:role/X").is_none());
    }

    #[test]
    fn rejects_empty_role_name() {
        assert!(parse_iam_role_arn("arn:aws:iam::123456789012:role/").is_none());
    }
}
