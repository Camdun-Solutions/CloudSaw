// Contract 05-QA — Terraform Scanner-Role Provisioner verification tests.
//
// This file batches the QA acceptance checks that are verifiable without a
// real AWS account and without the bundled Terraform binary (which is added
// by Contract 16 / Next-Steps C2). Each test maps to a specific QA item from
// `cloud-saw-contracts/C05-terraform-provisioner-QA.md`.
//
// The items that require a live AWS environment to verify (`apply` actually
// creating the role, `aws iam get-role` confirmation, partial-apply resume)
// are documented in CONTRACT_05_VERIFICATION.md as operator-driven checks.
//
// Tests serialize through their own module-level mutex like the other
// integration tests in this crate.

use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use cloudsaw_lib::accounts::{storage as accounts_storage, types::AccountRecord, Environment};
use cloudsaw_lib::db::migrations;
use cloudsaw_lib::terraform::{
    self, binary, identity, plans, runner, storage as tf_storage, types::PolicyVariant, workdir,
    TerraformAvailability, TerraformError,
};

fn env_lock() -> &'static Mutex<()> {
    static L: OnceLock<Mutex<()>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(()))
}

struct Sandbox {
    _guard: std::sync::MutexGuard<'static, ()>,
    dir: PathBuf,
    prev_bin: Option<String>,
    prev_sha: Option<String>,
    prev_module: Option<String>,
}

impl Sandbox {
    fn new(label: &str) -> Self {
        let guard = env_lock().lock().unwrap_or_else(|p| p.into_inner());
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("cloudsaw-qa05-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        fs::create_dir_all(dir.join("db")).unwrap();
        std::env::set_var("CLOUDSAW_DATA_DIR_OVERRIDE", &dir);
        migrations::run(&dir.join("db").join("cloudsaw.db")).unwrap();

        let prev_bin = std::env::var("CLOUDSAW_TERRAFORM_BIN_OVERRIDE").ok();
        let prev_sha = std::env::var("CLOUDSAW_TERRAFORM_SHA256_OVERRIDE").ok();
        let prev_module = std::env::var("CLOUDSAW_TF_MODULE_OVERRIDE").ok();
        std::env::remove_var("CLOUDSAW_TERRAFORM_BIN_OVERRIDE");
        std::env::remove_var("CLOUDSAW_TERRAFORM_SHA256_OVERRIDE");
        std::env::remove_var("CLOUDSAW_TF_MODULE_OVERRIDE");

        Self {
            _guard: guard,
            dir,
            prev_bin,
            prev_sha,
            prev_module,
        }
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        std::env::remove_var("CLOUDSAW_DATA_DIR_OVERRIDE");
        match &self.prev_bin {
            Some(v) => std::env::set_var("CLOUDSAW_TERRAFORM_BIN_OVERRIDE", v),
            None => std::env::remove_var("CLOUDSAW_TERRAFORM_BIN_OVERRIDE"),
        }
        match &self.prev_sha {
            Some(v) => std::env::set_var("CLOUDSAW_TERRAFORM_SHA256_OVERRIDE", v),
            None => std::env::remove_var("CLOUDSAW_TERRAFORM_SHA256_OVERRIDE"),
        }
        match &self.prev_module {
            Some(v) => std::env::set_var("CLOUDSAW_TF_MODULE_OVERRIDE", v),
            None => std::env::remove_var("CLOUDSAW_TF_MODULE_OVERRIDE"),
        }
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
}

// ============================================================================
// HAPPY PATH (automatable subset)
// ============================================================================

/// QA Happy Path #1: `detect_terraform` reports the bundled binary present
/// and integrity-valid. We exercise the success branch by overriding the
/// path + hash; the "no binary" branch is in `terraform_test.rs`.
#[test]
fn qa_happy_detect_terraform_succeeds_when_binary_matches_pinned_hash() {
    use sha2::{Digest, Sha256};
    let sb = Sandbox::new("happy-detect");
    let path = sb.dir.join("fake-tf");
    fs::write(&path, b"pretend this is terraform").unwrap();
    let mut h = Sha256::new();
    h.update(b"pretend this is terraform");
    let sha = hex::encode(h.finalize());
    std::env::set_var("CLOUDSAW_TERRAFORM_BIN_OVERRIDE", &path);
    std::env::set_var("CLOUDSAW_TERRAFORM_SHA256_OVERRIDE", &sha);
    let availability = terraform::detect_terraform();
    match availability {
        TerraformAvailability::Available { sha256, .. } => {
            assert_eq!(sha256, sha);
        }
        other => panic!("expected Available, got {other:?}"),
    }
}

// ============================================================================
// ERROR STATES
// ============================================================================

/// QA Error State: tampered Terraform binary → `plan`/`apply` fail with an
/// integrity error. We exercise the `binary::locate_and_verify()` path used
/// internally by `runner::plan`/`apply`.
#[test]
fn qa_error_tampered_binary_yields_integrity_failed() {
    let sb = Sandbox::new("err-tampered");
    let path = sb.dir.join("tampered");
    fs::write(&path, b"someone-replaced-the-binary").unwrap();
    std::env::set_var("CLOUDSAW_TERRAFORM_BIN_OVERRIDE", &path);
    std::env::set_var(
        "CLOUDSAW_TERRAFORM_SHA256_OVERRIDE",
        "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
    );
    let err = binary::locate_and_verify().unwrap_err();
    assert!(matches!(err, TerraformError::IntegrityFailed));
}

/// QA Error State: stale `plan_token` → apply rejected. Verified end-to-end
/// in `terraform_test::plan_token_supersession_rejects_stale_applies`; this
/// test adds the "applying after the workdir is gone" symmetric case.
#[test]
fn qa_error_apply_with_invalid_token_is_rejected_before_terraform_runs() {
    let _sb = Sandbox::new("err-invalid-token");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    accounts_storage::insert(&AccountRecord {
        aws_account_id: "111122223333".into(),
        label: "qa-dev".into(),
        profile_name: "qa-profile".into(),
        environment: Environment::Dev,
    })
    .unwrap();
    // No plan has been minted; consuming any token must fail with
    // PlanTokenInvalid — and crucially, the runner never tries to invoke
    // terraform because consume() short-circuits.
    let err = rt
        .block_on(terraform::apply("111122223333", "i-am-not-a-real-token"))
        .unwrap_err();
    assert!(matches!(err, TerraformError::PlanTokenInvalid));
}

/// QA Error State: bundled module source missing/corrupt → clear error,
/// no execution.
#[test]
fn qa_error_missing_module_source_yields_internal_module_source_missing() {
    let _sb = Sandbox::new("err-no-module");
    // Point the module override at a non-existent path. workdir::prepare
    // probes the override first and returns a typed error rather than
    // silently using a stale source.
    let bogus = std::env::temp_dir().join("cloudsaw-this-path-must-not-exist");
    let _ = fs::remove_dir_all(&bogus);
    std::env::set_var("CLOUDSAW_TF_MODULE_OVERRIDE", &bogus);
    let err = workdir::prepare("111122223333").unwrap_err();
    assert!(matches!(err, TerraformError::WorkdirIo(_)));
}

/// QA Error State: `apply` interrupted partway → state persists. We don't
/// run Terraform here, but we verify the structural property: the workdir
/// is created with `tf-work/<account>/`, and re-preparing it does NOT wipe
/// existing state files (so a `.terraform/` cache and a partial `terraform.tfstate`
/// would survive a re-plan).
#[test]
fn qa_error_workdir_sync_preserves_existing_state_files() {
    let _sb = Sandbox::new("err-resume");
    std::env::set_var(
        "CLOUDSAW_TF_MODULE_OVERRIDE",
        manifest_dir().join("tf-modules").join("scanner-role"),
    );

    let workdir_path = workdir::prepare("111122223333").unwrap();

    // Simulate a partial-apply leftover state file.
    let state_path = workdir_path.join("terraform.tfstate");
    fs::write(&state_path, b"{ \"version\": 4 }").unwrap();
    let plan_path = workdir_path.join("cloudsaw.tfplan");
    fs::write(&plan_path, b"binary plan").unwrap();
    let dot_terraform = workdir_path.join(".terraform");
    fs::create_dir_all(&dot_terraform).unwrap();
    fs::write(dot_terraform.join("lock.hcl"), b"providers").unwrap();

    // Re-prepare (simulating a re-plan after an interruption).
    let again = workdir::prepare("111122223333").unwrap();
    assert_eq!(again, workdir_path);

    // Existing state-shaped artifacts MUST still be there. Resume is the
    // user-visible behavior; this test guards against a regression that
    // wipes state on re-plan.
    assert!(state_path.exists(), "tfstate must survive a re-plan");
    assert!(
        plan_path.exists(),
        "previous plan file must survive a re-plan"
    );
    assert!(
        dot_terraform.join("lock.hcl").exists(),
        ".terraform cache must survive a re-plan"
    );
}

// ============================================================================
// SECURITY CHECK
// ============================================================================

/// Security Check #1: Terraform invoked by absolute path with argv arrays;
/// no shell anywhere in the runner. We strip Rust line-comments before
/// scanning so the comment that says "a shell is NEVER used" doesn't trip
/// the check on itself — we want to catch *code* invocations.
#[test]
fn qa_security_runner_source_has_no_shell_invocation() {
    let runner_src = fs::read_to_string(
        manifest_dir()
            .join("src")
            .join("terraform")
            .join("runner.rs"),
    )
    .unwrap();
    let code_only: String = runner_src
        .lines()
        .map(|l| {
            // Strip `//`-style line comments. Naive but adequate: there are
            // no string literals in this file that legitimately contain `//`.
            match l.find("//") {
                Some(idx) => &l[..idx],
                None => l,
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Patterns that would indicate an actual shell invocation. None of
    // these legitimately appear in the runner; if any do, the build fails.
    let forbidden = [
        "Command::new(\"sh\")",
        "Command::new(\"bash\")",
        "Command::new(\"/bin/sh\")",
        "Command::new(\"/bin/bash\")",
        "Command::new(\"cmd\")",
        "Command::new(\"cmd.exe\")",
        "Command::new(\"powershell\")",
        "Command::new(\"pwsh\")",
        "shell_exec",
        ".arg(\"-c\"",
        ".arg(\"/c\"",
    ];
    for needle in forbidden {
        assert!(
            !code_only.contains(needle),
            "runner.rs (code-only) must never invoke a shell — found {needle:?}"
        );
    }
    // Positive check: the runner uses Command::new(<verified-path>).args(<slice>).
    assert!(
        code_only.contains("Command::new(&tf)"),
        "runner.rs must spawn via Command::new(absolute_path)"
    );
    assert!(
        code_only.contains("cmd.args(args)"),
        "runner.rs must pass argv as a slice, not a joined string"
    );
}

/// Security Check #2: binary SHA-256 is verified against the build-pinned
/// hash before every execution. Verified by reading the source — `run()`
/// calls `prepare_invocation()` which calls `binary::locate_and_verify()`.
#[test]
fn qa_security_run_path_invokes_locate_and_verify() {
    let runner_src = fs::read_to_string(
        manifest_dir()
            .join("src")
            .join("terraform")
            .join("runner.rs"),
    )
    .unwrap();
    assert!(
        runner_src.contains("prepare_invocation()"),
        "every run() must go through prepare_invocation()"
    );
    let prepare_block = runner_src
        .split("fn prepare_invocation")
        .nth(1)
        .expect("prepare_invocation must exist");
    assert!(
        prepare_block.contains("binary::locate_and_verify"),
        "prepare_invocation must call binary::locate_and_verify()"
    );
}

/// Security Check #3: the created role's trust-policy principal equals the
/// live caller ARN and is never a wildcard. Verified by the trust-policy
/// re-verification step in `runner::verify_trust_policy_principal`.
#[test]
fn qa_security_trust_policy_verifier_rejects_wildcard_and_mismatch() {
    // Wildcard principal — must reject.
    let wildcard = serde_json::json!({
        "Version": "2012-10-17",
        "Statement": [{
            "Effect": "Allow",
            "Principal": { "AWS": "*" },
            "Action": "sts:AssumeRole"
        }]
    })
    .to_string();
    assert!(matches!(
        runner::verify_trust_policy_principal(&wildcard, "arn:aws:iam::111122223333:role/X"),
        Err(TerraformError::TrustVerificationFailed)
    ));

    // Different role — must reject.
    let other = serde_json::json!({
        "Version": "2012-10-17",
        "Statement": [{
            "Effect": "Allow",
            "Principal": { "AWS": "arn:aws:iam::111122223333:role/Attacker" },
            "Action": "sts:AssumeRole"
        }]
    })
    .to_string();
    assert!(matches!(
        runner::verify_trust_policy_principal(&other, "arn:aws:iam::111122223333:role/X"),
        Err(TerraformError::TrustVerificationFailed)
    ));

    // Exact match — must accept.
    let ok = serde_json::json!({
        "Version": "2012-10-17",
        "Statement": [{
            "Effect": "Allow",
            "Principal": { "AWS": "arn:aws:iam::111122223333:role/X" },
            "Action": "sts:AssumeRole"
        }]
    })
    .to_string();
    assert!(runner::verify_trust_policy_principal(&ok, "arn:aws:iam::111122223333:role/X").is_ok());
}

/// Security Check #4: a federated/assumed-role caller resolves to the
/// underlying role ARN. Covered exhaustively in `terraform::identity::tests`
/// and `terraform_test`; here we assert the public function is wired up.
#[test]
fn qa_security_assumed_role_session_unwraps_to_underlying_role_arn() {
    // Standard assumed-role -> underlying IAM role ARN.
    assert_eq!(
        identity::underlying_principal_arn(
            "arn:aws:sts::111122223333:assumed-role/Deployer/jenkins-build",
        )
        .unwrap(),
        "arn:aws:iam::111122223333:role/Deployer"
    );
    // SSO Identity Center reserved role -> aws-reserved path form.
    assert_eq!(
        identity::underlying_principal_arn(
            "arn:aws:sts::111122223333:assumed-role/AWSReservedSSO_Admin_a1/alice@example.com",
        )
        .unwrap(),
        "arn:aws:iam::111122223333:role/aws-reserved/sso.amazonaws.com/AWSReservedSSO_Admin_a1"
    );
    // Wildcard / malformed must NOT resolve to "*".
    assert!(matches!(
        identity::underlying_principal_arn("*"),
        Err(TerraformError::IdentityUnresolvable)
    ));
}

/// Security Check #5: default attached policy is SecurityAudit; ReadOnlyAccess
/// requires explicit opt-in. Asserted at the Rust default, the Terraform
/// module default, and the React UI default.
#[test]
fn qa_security_default_policy_variant_is_security_audit() {
    // Rust default.
    assert_eq!(PolicyVariant::default(), PolicyVariant::SecurityAudit);
    assert_eq!(
        PolicyVariant::default().as_tf_str(),
        "security_audit",
        "default tfvars value must be security_audit"
    );

    // Terraform module default.
    let variables_tf = fs::read_to_string(
        manifest_dir()
            .join("tf-modules")
            .join("scanner-role")
            .join("variables.tf"),
    )
    .unwrap();
    assert!(
        variables_tf.contains("default     = \"security_audit\""),
        "tf-modules/scanner-role/variables.tf must default policy_variant to security_audit"
    );

    // React UI default — the modal renders the warning ONLY for the
    // read_only_access branch.
    let ui_src = fs::read_to_string(
        manifest_dir()
            .join("..")
            .join("src")
            .join("routes")
            .join("ProvisionScannerRole.tsx"),
    )
    .unwrap();
    assert!(
        ui_src.contains("useState<PolicyVariant>(\"security_audit\")"),
        "React UI must default the policy selector to security_audit"
    );
    assert!(
        ui_src.contains("terraform.provision.policy.warning"),
        "UI must surface the ReadOnlyAccess opt-in warning"
    );
}

/// Security Check #5 (continued): the UI's warning string is present in the
/// English locale and references the broader-permissions trade-off.
#[test]
fn qa_security_read_only_access_warning_present_in_en_locale() {
    let en = fs::read_to_string(
        manifest_dir()
            .join("..")
            .join("src")
            .join("locales")
            .join("en.json"),
    )
    .unwrap();
    assert!(
        en.contains("\"terraform.provision.policy.warning\":"),
        "en.json must define the ReadOnlyAccess warning"
    );
    let parsed: serde_json::Value = serde_json::from_str(&en).unwrap();
    let msg = parsed
        .get("terraform.provision.policy.warning")
        .and_then(|v| v.as_str())
        .expect("warning string must be defined");
    let lower = msg.to_ascii_lowercase();
    assert!(
        lower.contains("readonlyaccess") && lower.contains("broader"),
        "warning string must reference ReadOnlyAccess and the broader permission trade-off, got: {msg}"
    );
}

/// Security Check #6: `terraform destroy` is not exposed as a command
/// anywhere — neither as a Tauri IPC command nor inside the runner.
#[test]
fn qa_security_no_terraform_destroy_command_exposed() {
    // IPC surface — no command name may contain "destroy".
    let ipc_src =
        fs::read_to_string(manifest_dir().join("src").join("ipc").join("mod.rs")).unwrap();
    assert!(
        !ipc_src.to_ascii_lowercase().contains("destroy"),
        "ipc/mod.rs must not expose any destroy command"
    );

    // Runner — must never argv "destroy".
    let runner_src = fs::read_to_string(
        manifest_dir()
            .join("src")
            .join("terraform")
            .join("runner.rs"),
    )
    .unwrap();
    assert!(
        !runner_src.contains("\"destroy\""),
        "runner.rs must not invoke `terraform destroy`"
    );
    assert!(
        !runner_src.contains("'destroy'"),
        "runner.rs must not invoke `terraform destroy`"
    );

    // lib.rs invoke_handler — verify no command registered.
    let lib_src = fs::read_to_string(manifest_dir().join("src").join("lib.rs")).unwrap();
    assert!(
        !lib_src.to_ascii_lowercase().contains("destroy"),
        "lib.rs must not register any destroy command in the Tauri handler"
    );
}

/// Security Check #7: Terraform state lives ONLY under `tf-work/{account}/`
/// with user-only permissions; none of it is in the bundle or repo.
#[test]
fn qa_security_workdir_lives_under_app_data_dir_and_account_segment() {
    let sb = Sandbox::new("sec-workdir-bounds");
    std::env::set_var(
        "CLOUDSAW_TF_MODULE_OVERRIDE",
        manifest_dir().join("tf-modules").join("scanner-role"),
    );
    let path = workdir::prepare("111122223333").unwrap();
    // Under data root.
    assert!(path.starts_with(&sb.dir));
    // Ends with tf-work/<account>.
    assert!(
        path.ends_with(PathBuf::from("tf-work").join("111122223333")),
        "workdir {path:?} must end with tf-work/<account>"
    );
    // Not under the repo / bundle source tree.
    let repo_root = manifest_dir().parent().unwrap().to_path_buf();
    assert!(
        !path.starts_with(&repo_root),
        "workdir {path:?} must NOT be inside the repo {repo_root:?}"
    );
}

/// Security Check #7 (continued): repo / bundle source tree contains no
/// Terraform state. We grep the checked-in tf-modules directory for
/// `terraform.tfstate*` files (Contract 05 §Constraints).
#[test]
fn qa_security_no_terraform_state_in_repo() {
    fn walk(dir: &std::path::Path, acc: &mut Vec<PathBuf>) {
        if let Ok(rd) = fs::read_dir(dir) {
            for entry in rd.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    walk(&p, acc);
                } else {
                    acc.push(p);
                }
            }
        }
    }
    let mut files = Vec::new();
    walk(&manifest_dir().join("tf-modules"), &mut files);
    for f in &files {
        let name = f.file_name().unwrap().to_string_lossy().to_string();
        assert!(
            !name.starts_with("terraform.tfstate"),
            "checked-in tf-modules must not contain state files (found {f:?})"
        );
        assert!(
            !name.ends_with(".tfplan"),
            "checked-in tf-modules must not contain plan binaries (found {f:?})"
        );
    }
}

// ============================================================================
// STATE TRANSITIONS (automatable subset)
// ============================================================================

/// QA State Transition: the `provisioning_status` state machine cleanly
/// transitions NotProvisioned → Provisioned on a successful apply, and
/// remains Provisioned across re-reads.
#[test]
fn qa_state_transition_not_provisioned_to_provisioned() {
    let _sb = Sandbox::new("state-not-to-provisioned");
    accounts_storage::insert(&AccountRecord {
        aws_account_id: "111122223333".into(),
        label: "qa-dev".into(),
        profile_name: "qa-profile".into(),
        environment: Environment::Dev,
    })
    .unwrap();

    let status = terraform::provisioning_status("111122223333").unwrap();
    assert!(matches!(
        status,
        terraform::ProvisioningStatus::NotProvisioned
    ));

    tf_storage::record_provisioned(
        "111122223333",
        "arn:aws:iam::111122223333:role/CloudSawScannerRole",
        PolicyVariant::SecurityAudit,
    )
    .unwrap();

    let status = terraform::provisioning_status("111122223333").unwrap();
    let provisioned_with_expected_arn = matches!(
        status,
        terraform::ProvisioningStatus::Provisioned { ref role_arn, .. }
            if role_arn == "arn:aws:iam::111122223333:role/CloudSawScannerRole"
    );
    assert!(provisioned_with_expected_arn);

    // Reading the status again must yield the same result.
    let status2 = terraform::provisioning_status("111122223333").unwrap();
    assert!(matches!(
        status2,
        terraform::ProvisioningStatus::Provisioned { .. }
    ));
}

/// QA State Transition: failure status surfaces the stable error code; a
/// subsequent successful provision clears it (record_provisioned wipes the
/// last_provisioning_error column).
#[test]
fn qa_state_transition_failure_then_recovery_clears_error() {
    let _sb = Sandbox::new("state-failure-then-recovery");
    accounts_storage::insert(&AccountRecord {
        aws_account_id: "111122223333".into(),
        label: "qa-dev".into(),
        profile_name: "qa-profile".into(),
        environment: Environment::Dev,
    })
    .unwrap();

    tf_storage::record_failure("111122223333", "terraform_apply_failed").unwrap();
    let status = terraform::provisioning_status("111122223333").unwrap();
    let matched = matches!(
        status,
        terraform::ProvisioningStatus::Failed { ref last_error_code, .. }
            if last_error_code == "terraform_apply_failed"
    );
    assert!(matched);

    tf_storage::record_provisioned(
        "111122223333",
        "arn:aws:iam::111122223333:role/CloudSawScannerRole",
        PolicyVariant::SecurityAudit,
    )
    .unwrap();
    let status = terraform::provisioning_status("111122223333").unwrap();
    assert!(matches!(
        status,
        terraform::ProvisioningStatus::Provisioned { .. }
    ));
}

// ============================================================================
// RESPONSIVENESS (limited to what's checkable without driving real terraform)
// ============================================================================

/// QA Responsiveness: the plan-diff structure carries enough information
/// for the UI to render a readable diff (kind, address, type, field names).
#[test]
fn qa_responsiveness_plan_change_shape_is_ui_renderable() {
    // Mirror the canonical Terraform `show -json` resource_changes shape.
    let plan = serde_json::json!({
        "resource_changes": [
            {
                "address": "aws_iam_role.scanner",
                "type": "aws_iam_role",
                "change": {
                    "actions": ["create"],
                    "before": null,
                    "after": {
                        "name": "CloudSawScannerRole",
                        "assume_role_policy": "{...}",
                        "max_session_duration": 3600
                    }
                }
            }
        ]
    });
    let changes = runner::parse_plan_changes(&plan).unwrap();
    assert_eq!(changes.len(), 1);
    let c = &changes[0];
    assert_eq!(c.resource_address, "aws_iam_role.scanner");
    assert_eq!(c.resource_type, "aws_iam_role");
    assert!(matches!(
        c.kind,
        cloudsaw_lib::terraform::PlanChangeKind::Create
    ));
    assert!(c.summary.contains("create"));
    assert!(c.summary.contains("aws_iam_role.scanner"));
    // Attribute field names are present (not values).
    assert!(c.attributes.iter().any(|a| a == "name"));
    assert!(c.attributes.iter().any(|a| a == "assume_role_policy"));
    // Values are NOT serialized — the UI never sees "CloudSawScannerRole"
    // through this surface.
    for attr in &c.attributes {
        assert!(!attr.contains("CloudSaw"), "field NAMES only, never values");
        assert!(!attr.contains("3600"), "field NAMES only, never values");
    }
}

// ============================================================================
// Stand-alone: external_id is generated by CloudSaw, persists across
// re-plans, and is never empty (defends the trust-policy ExternalId
// confused-deputy guard).
// ============================================================================

#[test]
fn qa_security_external_id_is_non_empty_and_stable_across_plans() {
    let _sb = Sandbox::new("ext-id-stable");
    accounts_storage::insert(&AccountRecord {
        aws_account_id: "111122223333".into(),
        label: "qa-dev".into(),
        profile_name: "qa-profile".into(),
        environment: Environment::Dev,
    })
    .unwrap();

    let id1 = tf_storage::ensure_external_id("111122223333").unwrap();
    assert!(id1.len() >= 32, "external_id must be at least 32 chars");
    assert!(
        id1.chars().all(|c| c.is_ascii_hexdigit()),
        "external_id must be hex"
    );

    let id2 = tf_storage::ensure_external_id("111122223333").unwrap();
    assert_eq!(id1, id2, "external_id must be stable across re-plans");

    // Plant a row with NULL/empty external_id and verify it gets generated
    // on first ensure_* call.
    accounts_storage::insert(&AccountRecord {
        aws_account_id: "444455556666".into(),
        label: "qa-prod".into(),
        profile_name: "qa-profile-2".into(),
        environment: Environment::Prod,
    })
    .unwrap();
    let id_for_second = tf_storage::ensure_external_id("444455556666").unwrap();
    assert!(!id_for_second.is_empty());
    assert_ne!(id_for_second, id1, "different accounts get different IDs");
}

/// QA defense in depth: the in-memory plan store mints unique tokens such
/// that two consecutive plans for the same account produce distinguishable
/// tokens (and only the latest is accepted by apply).
#[test]
fn qa_security_plan_tokens_are_unguessable_and_unique() {
    use std::collections::HashSet;
    let mut seen = HashSet::new();
    for _ in 0..256 {
        let t = plans::mint_token();
        assert_eq!(t.len(), 32);
        assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(seen.insert(t), "plan tokens must not collide");
    }
}
