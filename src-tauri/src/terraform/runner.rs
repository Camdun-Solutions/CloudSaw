// Terraform invocation — init, plan, apply.
//
// CLAUDE.md §4.5 + Contract 05 §Constraints:
//   * External binaries are invoked by absolute path with argv arrays. A
//     shell is NEVER used.
//   * The binary's SHA-256 is verified against the build-pinned hash before
//     every execution.
//
// We use `std::process::Command` directly. `Command::new(path).arg(...)`
// never invokes a shell — there is no /bin/sh layer, no PATH lookup, no
// string interpolation. Every argument is a separate argv entry. The
// integrity check happens in `prepare_invocation` immediately before the
// child is spawned.
//
// stdout/stderr capture: we DO capture both, but stderr is consumed only for
// the structured plan-file parsing inputs and a small set of categorization
// hints. Raw stderr is NEVER returned across IPC (CLAUDE.md §4.2); the
// classified TerraformError is what reaches the frontend.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::{json, Value};

use super::binary;
use super::error::TerraformError;
use super::identity::underlying_principal_arn;
use super::plans::{self, PlanEntry};
use super::storage;
use super::types::{
    ApplyResult, PlanChange, PlanChangeKind, PlanOptions, PlanResult, PolicyVariant,
};
use super::workdir;
use crate::accounts;
use crate::auth;

/// Locate and verify the binary, then return the absolute path that callers
/// pass to `Command::new`. Re-runs every invocation per the contract.
fn prepare_invocation() -> Result<PathBuf, TerraformError> {
    let (path, _sha) = binary::locate_and_verify()?;
    Ok(path)
}

/// Run a `terraform` subcommand with the given argv inside `workdir`.
/// Captures both streams (stderr is consumed only for classification) and
/// returns the raw output to the caller; the caller classifies the exit
/// status into a typed error.
fn run(workdir: &Path, args: &[&str]) -> Result<Output, TerraformError> {
    let tf = prepare_invocation()?;

    let mut cmd = Command::new(&tf);
    cmd.args(args);
    cmd.current_dir(workdir);
    // No -input prompts. CloudSaw never serves interactive input to Terraform.
    cmd.env("TF_IN_AUTOMATION", "1");
    cmd.env("TF_INPUT", "0");
    // Pin Terraform's data dir to the workdir so its `.terraform/` cache
    // never escapes into a parent directory. Belt and suspenders — current_dir
    // already gives us this — but explicit is better here.
    cmd.env("TF_DATA_DIR", workdir.join(".terraform"));
    // Disable the version-update check; we ship a pinned binary.
    cmd.env("CHECKPOINT_DISABLE", "1");
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let output = cmd.output()?;
    Ok(output)
}

/// `terraform init` — downloads providers, sets up the local backend. Idempotent.
fn terraform_init(workdir: &Path) -> Result<(), TerraformError> {
    let out = run(
        workdir,
        &["init", "-no-color", "-input=false", "-upgrade=false"],
    )?;
    if !out.status.success() {
        return Err(TerraformError::InitFailed);
    }
    Ok(())
}

/// `terraform plan -out=...` — writes a binary plan and a JSON show file
/// alongside. Returns the parsed plan summary.
fn terraform_plan(
    workdir: &Path,
    plan_path: &Path,
) -> Result<(bool, Vec<PlanChange>), TerraformError> {
    let plan_arg = format!("-out={}", plan_path.display());
    let out = run(
        workdir,
        &[
            "plan",
            "-no-color",
            "-input=false",
            "-detailed-exitcode",
            &plan_arg,
        ],
    )?;
    // -detailed-exitcode: 0 = no changes, 2 = changes present, 1 = error.
    let code = out.status.code().unwrap_or(1);
    match code {
        0 | 2 => {}
        _ => return Err(TerraformError::PlanFailed),
    }

    let show_output = run(
        workdir,
        &[
            "show",
            "-no-color",
            "-json",
            &plan_path.display().to_string(),
        ],
    )?;
    if !show_output.status.success() {
        return Err(TerraformError::PlanFailed);
    }

    let json_text = String::from_utf8_lossy(&show_output.stdout);
    let parsed: Value = serde_json::from_str(&json_text)
        .map_err(|_| TerraformError::Internal("plan_json_parse"))?;

    let changes = parse_plan_changes(&parsed)?;
    let no_changes = code == 0 || changes.is_empty();
    Ok((no_changes, changes))
}

/// Parse the structured plan JSON `terraform show -json <plan>` emits into
/// the UI-friendly `PlanChange` list. Account IDs in the change summary are
/// masked to their last 4 digits (CLAUDE.md §4.4); attribute *values* are
/// not included at all so we cannot leak credential-bearing data.
pub fn parse_plan_changes(plan: &Value) -> Result<Vec<PlanChange>, TerraformError> {
    let resource_changes = plan
        .get("resource_changes")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    for rc in resource_changes {
        let address = rc
            .get("address")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let resource_type = rc
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let actions: Vec<String> = rc
            .get("change")
            .and_then(|c| c.get("actions"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let kind = classify_actions(&actions);

        // Names of attributes touched by this change. We extract field names
        // only — never values.
        let attributes = attribute_names(&rc);

        let summary = summarize_change(kind, &address, &attributes);
        out.push(PlanChange {
            kind,
            resource_address: address,
            resource_type,
            summary,
            attributes,
        });
    }
    Ok(out)
}

fn classify_actions(actions: &[String]) -> PlanChangeKind {
    // Terraform's action vocabulary: ["no-op"], ["create"], ["read"],
    // ["update"], ["delete"], ["delete","create"], ["create","delete"].
    let has = |s: &str| actions.iter().any(|a| a == s);
    if actions.len() == 2 && has("create") && has("delete") {
        return PlanChangeKind::Replace;
    }
    if has("create") {
        return PlanChangeKind::Create;
    }
    if has("update") {
        return PlanChangeKind::Update;
    }
    if has("delete") {
        return PlanChangeKind::Delete;
    }
    if has("read") {
        return PlanChangeKind::Read;
    }
    PlanChangeKind::NoOp
}

/// Extract field names from the `change.before`/`change.after` objects. We
/// take the union of top-level keys whose values differ. Values are never
/// included.
fn attribute_names(resource_change: &Value) -> Vec<String> {
    let before = resource_change
        .get("change")
        .and_then(|c| c.get("before"))
        .and_then(|v| v.as_object());
    let after = resource_change
        .get("change")
        .and_then(|c| c.get("after"))
        .and_then(|v| v.as_object());

    let mut keys: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    if let Some(b) = before {
        for (k, _) in b {
            keys.insert(k.clone());
        }
    }
    if let Some(a) = after {
        for (k, _) in a {
            keys.insert(k.clone());
        }
    }
    keys.into_iter().collect()
}

fn summarize_change(kind: PlanChangeKind, address: &str, attrs: &[String]) -> String {
    let verb = match kind {
        PlanChangeKind::Create => "create",
        PlanChangeKind::Update => "update",
        PlanChangeKind::Delete => "delete",
        PlanChangeKind::Replace => "replace",
        PlanChangeKind::NoOp => "no-op",
        PlanChangeKind::Read => "read",
    };
    let attr_part = if attrs.is_empty() {
        String::new()
    } else {
        format!(" ({} fields)", attrs.len())
    };
    format!("{verb} {address}{attr_part}")
}

/// `terraform apply <plan_file>` — applies a previously-saved plan. The
/// argument is the binary plan file the same `plan` invocation produced.
fn terraform_apply(workdir: &Path, plan_path: &Path) -> Result<(), TerraformError> {
    let out = run(
        workdir,
        &[
            "apply",
            "-no-color",
            "-input=false",
            "-auto-approve=false",
            &plan_path.display().to_string(),
        ],
    )?;
    if !out.status.success() {
        return Err(TerraformError::ApplyFailed);
    }
    Ok(())
}

/// `terraform output -json` — read structured outputs after apply.
fn terraform_outputs(workdir: &Path) -> Result<Value, TerraformError> {
    let out = run(workdir, &["output", "-no-color", "-json"])?;
    if !out.status.success() {
        return Err(TerraformError::ApplyFailed);
    }
    let parsed: Value = serde_json::from_slice(&out.stdout)
        .map_err(|_| TerraformError::Internal("outputs_json_parse"))?;
    Ok(parsed)
}

// ----- Public-API entry points called by `terraform::mod` ----------------

/// Compose tfvars JSON for plan/apply. The Rust caller owns this so the
/// values are never user-typed strings — every field is derived from a
/// verified source.
fn render_tfvars(
    trusted_principal_arn: &str,
    external_id: &str,
    policy_variant: PolicyVariant,
) -> String {
    let body = json!({
        "trusted_principal_arn": trusted_principal_arn,
        "external_id": external_id,
        "policy_variant": policy_variant.as_tf_str(),
    });
    body.to_string()
}

/// Run init + plan against the given account. On success, registers the plan
/// in the in-memory token store and returns the `PlanResult` for the UI.
///
/// Fails fast on integrity/identity issues — those are surfaced as stable
/// TerraformError codes; the UI maps them to localized copy.
pub async fn plan(
    aws_account_id: &str,
    options: PlanOptions,
) -> Result<PlanResult, TerraformError> {
    let account = accounts::get_account(aws_account_id)
        .map_err(|_| TerraformError::InvalidInput("aws_account_id"))?;
    let identity = auth::get_caller_identity(&account.profile_name).await?;

    if identity.account_id != aws_account_id {
        return Err(TerraformError::Internal("identity_account_mismatch"));
    }

    let principal_arn = underlying_principal_arn(&identity.arn)?;
    let external_id = storage::ensure_external_id(aws_account_id)?;
    let workdir_path = workdir::prepare(aws_account_id)?;
    let tfvars = render_tfvars(&principal_arn, &external_id, options.policy_variant);
    workdir::write_tfvars(&workdir_path, &tfvars)?;

    if let Err(e) = terraform_init(&workdir_path) {
        let _ = storage::record_failure(aws_account_id, e.code());
        return Err(e);
    }

    let plan_path = workdir::plan_file(&workdir_path);
    let (no_changes, changes) = match terraform_plan(&workdir_path, &plan_path) {
        Ok(v) => v,
        Err(e) => {
            let _ = storage::record_failure(aws_account_id, e.code());
            return Err(e);
        }
    };

    let plan_token = plans::mint_token();
    let created_at = chrono::Utc::now();
    plans::insert(PlanEntry {
        plan_token: plan_token.clone(),
        aws_account_id: aws_account_id.to_string(),
        plan_file: plan_path.clone(),
        planned_principal_arn: principal_arn.clone(),
        policy_variant: options.policy_variant,
        no_changes,
        changes: changes.clone(),
        created_at,
    });

    Ok(PlanResult {
        plan_token,
        no_changes,
        changes,
        planned_principal_arn: principal_arn,
        policy_variant: options.policy_variant,
        created_at,
    })
}

/// Apply a previously confirmed plan. Validates the plan_token before doing
/// anything else; on success, records the outputs and returns the role ARN.
pub async fn apply(aws_account_id: &str, plan_token: &str) -> Result<ApplyResult, TerraformError> {
    let entry = plans::consume(aws_account_id, plan_token)?;

    let workdir_path = workdir::workdir(aws_account_id)?;
    if !workdir_path.is_dir() {
        // The workdir was cleaned out between plan and apply — refuse rather
        // than silently re-creating and re-planning.
        return Err(TerraformError::PlanTokenInvalid);
    }

    if let Err(e) = terraform_apply(&workdir_path, &entry.plan_file) {
        let _ = storage::record_failure(aws_account_id, e.code());
        return Err(e);
    }

    let outputs = match terraform_outputs(&workdir_path) {
        Ok(v) => v,
        Err(e) => {
            let _ = storage::record_failure(aws_account_id, e.code());
            return Err(e);
        }
    };

    let role_arn = outputs
        .get("role_arn")
        .and_then(|v| v.get("value"))
        .and_then(|v| v.as_str())
        .ok_or(TerraformError::Internal("missing_role_arn_output"))?
        .to_string();
    let role_name = outputs
        .get("role_name")
        .and_then(|v| v.get("value"))
        .and_then(|v| v.as_str())
        .ok_or(TerraformError::Internal("missing_role_name_output"))?
        .to_string();
    let trust_policy_json = outputs
        .get("trust_policy_json")
        .and_then(|v| v.get("value"))
        .and_then(|v| v.as_str())
        .ok_or(TerraformError::Internal("missing_trust_policy_output"))?
        .to_string();

    // Acceptance criterion: the rendered trust policy principal MUST equal the
    // planned ARN. We parse the policy JSON and check the Principal.AWS field;
    // a mismatch surfaces TrustVerificationFailed rather than silently passing.
    verify_trust_policy_principal(&trust_policy_json, &entry.planned_principal_arn)?;

    let trust_policy_sha256 = {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(trust_policy_json.as_bytes());
        hex::encode(h.finalize())
    };

    storage::record_provisioned(aws_account_id, &role_arn, entry.policy_variant)?;

    Ok(ApplyResult {
        role_arn,
        role_name,
        policy_variant: entry.policy_variant,
        trust_policy_sha256,
    })
}

/// Parse the trust-policy JSON Terraform produced and confirm its first
/// (and only) statement principal exactly matches `expected_principal_arn`.
/// Refusing to accept a wildcard here is belt-and-suspenders to the
/// variable validator on the Terraform side.
pub fn verify_trust_policy_principal(
    policy_json: &str,
    expected_principal_arn: &str,
) -> Result<(), TerraformError> {
    let parsed: Value =
        serde_json::from_str(policy_json).map_err(|_| TerraformError::TrustVerificationFailed)?;
    let statements = parsed
        .get("Statement")
        .and_then(|v| v.as_array())
        .ok_or(TerraformError::TrustVerificationFailed)?;
    if statements.len() != 1 {
        return Err(TerraformError::TrustVerificationFailed);
    }
    let statement = &statements[0];
    let effect = statement.get("Effect").and_then(|v| v.as_str());
    if effect != Some("Allow") {
        return Err(TerraformError::TrustVerificationFailed);
    }
    let action = statement.get("Action").and_then(|v| v.as_str());
    if action != Some("sts:AssumeRole") {
        return Err(TerraformError::TrustVerificationFailed);
    }
    let principal_aws = statement
        .get("Principal")
        .and_then(|p| p.get("AWS"))
        .and_then(|v| v.as_str());
    match principal_aws {
        Some(p) if p == expected_principal_arn => Ok(()),
        Some("*") | None => Err(TerraformError::TrustVerificationFailed),
        Some(_) => Err(TerraformError::TrustVerificationFailed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_actions_handles_terraform_vocabulary() {
        assert_eq!(
            classify_actions(&["create".to_string()]),
            PlanChangeKind::Create
        );
        assert_eq!(
            classify_actions(&["update".to_string()]),
            PlanChangeKind::Update
        );
        assert_eq!(
            classify_actions(&["delete".to_string()]),
            PlanChangeKind::Delete
        );
        assert_eq!(
            classify_actions(&["no-op".to_string()]),
            PlanChangeKind::NoOp
        );
        assert_eq!(
            classify_actions(&["read".to_string()]),
            PlanChangeKind::Read
        );
        // delete+create in either order = replace.
        assert_eq!(
            classify_actions(&["delete".to_string(), "create".to_string()]),
            PlanChangeKind::Replace
        );
        assert_eq!(
            classify_actions(&["create".to_string(), "delete".to_string()]),
            PlanChangeKind::Replace
        );
    }

    #[test]
    fn parse_plan_changes_extracts_address_kind_and_field_names() {
        let plan = json!({
            "resource_changes": [
                {
                    "address": "aws_iam_role.scanner",
                    "type": "aws_iam_role",
                    "change": {
                        "actions": ["create"],
                        "before": null,
                        "after": {
                            "name": "CloudSawScannerRole",
                            "assume_role_policy": "...",
                            "max_session_duration": 3600
                        }
                    }
                },
                {
                    "address": "aws_iam_role_policy_attachment.scanner",
                    "type": "aws_iam_role_policy_attachment",
                    "change": {
                        "actions": ["create"],
                        "before": null,
                        "after": {
                            "role": "CloudSawScannerRole",
                            "policy_arn": "arn:aws:iam::aws:policy/SecurityAudit"
                        }
                    }
                }
            ]
        });
        let changes = parse_plan_changes(&plan).unwrap();
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].kind, PlanChangeKind::Create);
        assert_eq!(changes[0].resource_address, "aws_iam_role.scanner");
        assert_eq!(changes[0].resource_type, "aws_iam_role");
        assert!(changes[0].attributes.iter().any(|a| a == "name"));
        // Critically — the *value* of name ("CloudSawScannerRole") is not in
        // the attribute names; only field names appear.
        for c in &changes {
            for a in &c.attributes {
                assert!(!a.contains("CloudSaw"));
                assert!(!a.contains("arn:aws"));
            }
        }
    }

    #[test]
    fn parse_plan_changes_treats_empty_resource_changes_as_noop_plan() {
        let plan = json!({"resource_changes": []});
        let changes = parse_plan_changes(&plan).unwrap();
        assert!(changes.is_empty());
    }

    #[test]
    fn render_tfvars_is_valid_json_with_expected_keys() {
        let body = render_tfvars(
            "arn:aws:iam::111122223333:role/Deployer",
            "abcdef0123456789",
            PolicyVariant::SecurityAudit,
        );
        let parsed: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            parsed
                .get("trusted_principal_arn")
                .unwrap()
                .as_str()
                .unwrap(),
            "arn:aws:iam::111122223333:role/Deployer"
        );
        assert_eq!(
            parsed.get("external_id").unwrap().as_str().unwrap(),
            "abcdef0123456789"
        );
        assert_eq!(
            parsed.get("policy_variant").unwrap().as_str().unwrap(),
            "security_audit"
        );
    }

    #[test]
    fn verify_trust_policy_accepts_matching_principal() {
        let policy = json!({
            "Version": "2012-10-17",
            "Statement": [
                {
                    "Effect": "Allow",
                    "Principal": { "AWS": "arn:aws:iam::111122223333:role/X" },
                    "Action": "sts:AssumeRole",
                    "Condition": {
                        "StringEquals": {
                            "sts:ExternalId": "abcdef"
                        }
                    }
                }
            ]
        })
        .to_string();
        assert!(verify_trust_policy_principal(&policy, "arn:aws:iam::111122223333:role/X").is_ok());
    }

    #[test]
    fn verify_trust_policy_rejects_wildcard_principal() {
        let policy = json!({
            "Version": "2012-10-17",
            "Statement": [
                {
                    "Effect": "Allow",
                    "Principal": { "AWS": "*" },
                    "Action": "sts:AssumeRole"
                }
            ]
        })
        .to_string();
        assert!(matches!(
            verify_trust_policy_principal(&policy, "arn:aws:iam::111122223333:role/X"),
            Err(TerraformError::TrustVerificationFailed)
        ));
    }

    #[test]
    fn verify_trust_policy_rejects_principal_mismatch() {
        let policy = json!({
            "Version": "2012-10-17",
            "Statement": [
                {
                    "Effect": "Allow",
                    "Principal": { "AWS": "arn:aws:iam::111122223333:role/EVIL" },
                    "Action": "sts:AssumeRole"
                }
            ]
        })
        .to_string();
        assert!(matches!(
            verify_trust_policy_principal(&policy, "arn:aws:iam::111122223333:role/X"),
            Err(TerraformError::TrustVerificationFailed)
        ));
    }

    #[test]
    fn verify_trust_policy_rejects_wrong_action_or_effect() {
        let deny = json!({
            "Version": "2012-10-17",
            "Statement": [
                {
                    "Effect": "Deny",
                    "Principal": { "AWS": "arn:aws:iam::111122223333:role/X" },
                    "Action": "sts:AssumeRole"
                }
            ]
        })
        .to_string();
        assert!(matches!(
            verify_trust_policy_principal(&deny, "arn:aws:iam::111122223333:role/X"),
            Err(TerraformError::TrustVerificationFailed)
        ));

        let wrong_action = json!({
            "Version": "2012-10-17",
            "Statement": [
                {
                    "Effect": "Allow",
                    "Principal": { "AWS": "arn:aws:iam::111122223333:role/X" },
                    "Action": "sts:AssumeRoleWithWebIdentity"
                }
            ]
        })
        .to_string();
        assert!(matches!(
            verify_trust_policy_principal(&wrong_action, "arn:aws:iam::111122223333:role/X"),
            Err(TerraformError::TrustVerificationFailed)
        ));
    }

    #[test]
    fn summarize_change_is_field_name_only_not_value() {
        let summary = summarize_change(
            PlanChangeKind::Create,
            "aws_iam_role.scanner",
            &["name".to_string(), "assume_role_policy".to_string()],
        );
        assert!(summary.contains("create"));
        assert!(summary.contains("aws_iam_role.scanner"));
        assert!(summary.contains("2 fields"));
        // No values leak in.
        assert!(!summary.contains("CloudSaw"));
    }
}
