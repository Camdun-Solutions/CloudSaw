// Integration tests for the terraform module (Contract 05).
//
// We can't actually invoke `terraform plan` against AWS from a unit test
// matrix, so the integration tests focus on the surfaces Contract 05 calls
// out as testable without network:
//
//   * `detect_terraform` returns the right status in three scenarios:
//     missing binary, present-and-matched binary, present-but-tampered.
//   * Trust-policy verification correctly accepts the planned principal and
//     rejects wildcards / mismatches.
//   * The plan-token store enforces freshness across plan supersession.
//   * The per-account workdir is created with the right module files and
//     never escapes the data root.
//   * `provisioning_status` walks the three-state machine the storage layer
//     exposes.
//
// The harness mirrors the accounts integration-test sandbox: per-test
// temporary data dir via `CLOUDSAW_DATA_DIR_OVERRIDE`, real migration run,
// real SQLite. Network-touching paths (the runner's `terraform plan` itself)
// are out of scope here — the contract pairs with a separate QA contract
// that drives those against a real AWS account.

use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use cloudsaw_lib::accounts::storage as accounts_storage;
use cloudsaw_lib::accounts::{self, types::AccountRecord, Environment};
use cloudsaw_lib::db::migrations;
use cloudsaw_lib::errors::AppError;
use cloudsaw_lib::terraform::{
    self, binary, plans, runner, storage as tf_storage, types::PolicyVariant, ProvisioningStatus,
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
        let dir = std::env::temp_dir().join(format!("cloudsaw-terraform-{label}-{nanos}"));
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

        plans::_clear_for_tests();

        Self {
            _guard: guard,
            dir,
            prev_bin,
            prev_sha,
            prev_module,
        }
    }

    fn data_dir(&self) -> &PathBuf {
        &self.dir
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        std::env::remove_var("CLOUDSAW_DATA_DIR_OVERRIDE");
        plans::_clear_for_tests();
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

fn write_fake_binary(sb: &Sandbox, content: &[u8]) -> (PathBuf, String) {
    use sha2::{Digest, Sha256};
    let path = sb.data_dir().join("fake-terraform.bin");
    fs::write(&path, content).unwrap();
    let mut h = Sha256::new();
    h.update(content);
    let sha = hex::encode(h.finalize());
    (path, sha)
}

fn seed_account(label: &str, profile: &str, aws_id: &str) -> AccountRecord {
    AccountRecord {
        aws_account_id: aws_id.to_string(),
        label: label.to_string(),
        profile_name: profile.to_string(),
        environment: Environment::Dev,
    }
}

// --- detect_terraform ----------------------------------------------------

#[test]
fn detect_terraform_reports_missing_when_no_binary() {
    let _sb = Sandbox::new("detect-missing");
    let availability = terraform::detect_terraform();
    assert!(
        matches!(availability, TerraformAvailability::Missing),
        "no binary on disk must surface as Missing, got {availability:?}"
    );
}

#[test]
fn detect_terraform_reports_available_when_hash_matches() {
    let sb = Sandbox::new("detect-ok");
    let (path, sha) = write_fake_binary(&sb, b"this is a terraform stand-in for tests");
    std::env::set_var("CLOUDSAW_TERRAFORM_BIN_OVERRIDE", &path);
    std::env::set_var("CLOUDSAW_TERRAFORM_SHA256_OVERRIDE", &sha);
    let availability = terraform::detect_terraform();
    match availability {
        TerraformAvailability::Available { sha256, .. } => {
            assert_eq!(sha256.to_ascii_lowercase(), sha);
        }
        other => panic!("expected Available, got {other:?}"),
    }
}

#[test]
fn detect_terraform_reports_integrity_failed_when_binary_tampered() {
    let sb = Sandbox::new("detect-tampered");
    let (path, _sha) = write_fake_binary(&sb, b"original-binary");
    // Pin a hash that does NOT match the file — same effect as tampering.
    std::env::set_var("CLOUDSAW_TERRAFORM_BIN_OVERRIDE", &path);
    std::env::set_var(
        "CLOUDSAW_TERRAFORM_SHA256_OVERRIDE",
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
    let availability = terraform::detect_terraform();
    assert!(
        matches!(availability, TerraformAvailability::IntegrityFailed),
        "tampered binary must surface as IntegrityFailed, got {availability:?}"
    );
}

#[test]
fn locate_and_verify_returns_typed_errors_in_each_failure_mode() {
    let sb = Sandbox::new("locate-and-verify");

    // No binary, no pin -> NotBundled.
    let err = binary::locate_and_verify().unwrap_err();
    assert!(matches!(err, TerraformError::NotBundled));

    // Binary present but no pin -> NotBundled (verify_sha256 path).
    let (path, _sha) = write_fake_binary(&sb, b"x");
    std::env::set_var("CLOUDSAW_TERRAFORM_BIN_OVERRIDE", &path);
    let err = binary::locate_and_verify().unwrap_err();
    assert!(
        matches!(err, TerraformError::NotBundled),
        "no pinned hash must surface as NotBundled, got {err:?}"
    );

    // Binary present, wrong pin -> IntegrityFailed.
    std::env::set_var(
        "CLOUDSAW_TERRAFORM_SHA256_OVERRIDE",
        "1111111111111111111111111111111111111111111111111111111111111111",
    );
    let err = binary::locate_and_verify().unwrap_err();
    assert!(matches!(err, TerraformError::IntegrityFailed));

    // Correct pin -> Ok.
    let (path2, sha2) = write_fake_binary(&sb, b"matched");
    std::env::set_var("CLOUDSAW_TERRAFORM_BIN_OVERRIDE", &path2);
    std::env::set_var("CLOUDSAW_TERRAFORM_SHA256_OVERRIDE", &sha2);
    let (p, s) = binary::locate_and_verify().unwrap();
    assert_eq!(p, path2);
    assert_eq!(s, sha2);
}

// --- provisioning_status -------------------------------------------------

#[test]
fn provisioning_status_returns_not_provisioned_for_a_fresh_account() {
    let _sb = Sandbox::new("status-fresh");
    accounts_storage::insert(&seed_account("dev", "dev-profile", "111122223333")).unwrap();
    let status = terraform::provisioning_status("111122223333").unwrap();
    assert!(matches!(status, ProvisioningStatus::NotProvisioned));
}

#[test]
fn provisioning_status_reports_provisioned_after_record_provisioned() {
    let _sb = Sandbox::new("status-provisioned");
    accounts_storage::insert(&seed_account("dev", "dev-profile", "111122223333")).unwrap();
    tf_storage::record_provisioned(
        "111122223333",
        "arn:aws:iam::111122223333:role/CloudSawScannerRole",
        PolicyVariant::SecurityAudit,
    )
    .unwrap();
    let status = terraform::provisioning_status("111122223333").unwrap();
    match status {
        ProvisioningStatus::Provisioned {
            role_arn,
            policy_variant,
            ..
        } => {
            assert_eq!(
                role_arn,
                "arn:aws:iam::111122223333:role/CloudSawScannerRole"
            );
            assert_eq!(policy_variant, PolicyVariant::SecurityAudit);
        }
        other => panic!("expected Provisioned, got {other:?}"),
    }
}

#[test]
fn provisioning_status_reports_failed_after_record_failure() {
    let _sb = Sandbox::new("status-failed");
    accounts_storage::insert(&seed_account("dev", "dev-profile", "111122223333")).unwrap();
    tf_storage::record_failure("111122223333", "terraform_apply_failed").unwrap();
    let status = terraform::provisioning_status("111122223333").unwrap();
    match status {
        ProvisioningStatus::Failed {
            last_error_code, ..
        } => {
            assert_eq!(last_error_code, "terraform_apply_failed");
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

// --- external_id persistence ---------------------------------------------

#[test]
fn external_id_is_generated_once_and_stable() {
    let _sb = Sandbox::new("external-id");
    accounts_storage::insert(&seed_account("dev", "dev-profile", "111122223333")).unwrap();

    let id1 = tf_storage::ensure_external_id("111122223333").unwrap();
    let id2 = tf_storage::ensure_external_id("111122223333").unwrap();
    assert_eq!(id1, id2, "external_id must be stable across repeated calls");
    assert_eq!(id1.len(), 32);
    assert!(id1.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn ensure_external_id_for_missing_account_fails() {
    let _sb = Sandbox::new("external-id-missing");
    let err = tf_storage::ensure_external_id("999988887777").unwrap_err();
    assert!(matches!(
        err,
        TerraformError::InvalidInput("aws_account_id")
    ));
}

// --- workdir preparation -------------------------------------------------

#[test]
fn workdir_prepare_copies_module_files_and_stays_under_data_dir() {
    let sb = Sandbox::new("workdir-prepare");
    // Point the module override at our checked-in module source so the test
    // is hermetic regardless of where the tests run from.
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let module_src = PathBuf::from(&manifest_dir)
        .join("tf-modules")
        .join("scanner-role");
    std::env::set_var("CLOUDSAW_TF_MODULE_OVERRIDE", &module_src);

    let workdir = terraform::workdir::prepare("111122223333").unwrap();

    // The workdir MUST be inside the data root.
    assert!(
        workdir.starts_with(sb.data_dir()),
        "workdir {workdir:?} escaped data dir {:?}",
        sb.data_dir()
    );
    assert!(workdir.ends_with(PathBuf::from("tf-work").join("111122223333")));

    // Module files must be present.
    for expected in ["main.tf", "variables.tf", "outputs.tf", "versions.tf"] {
        let p = workdir.join(expected);
        assert!(p.exists(), "expected {expected} in workdir");
    }
}

#[test]
fn workdir_prepare_rejects_malformed_account_id() {
    let _sb = Sandbox::new("workdir-bad-id");
    for bad in [
        "",
        "12345",
        "abcdefghijkl",
        "../../etc/passwd",
        "12345678901a",
    ] {
        let err = terraform::workdir::prepare(bad).unwrap_err();
        assert!(
            matches!(err, TerraformError::InvalidInput("aws_account_id")),
            "malformed id {bad:?} must be rejected, got {err:?}"
        );
    }
}

// --- plan-token freshness end-to-end --------------------------------------

#[test]
fn plan_token_supersession_rejects_stale_applies() {
    let _sb = Sandbox::new("plan-supersession");
    let account = "111122223333";

    // Plant a synthetic plan entry — we exercise the supersession rule
    // without needing the real Terraform invocation.
    plans::insert(plans::PlanEntry {
        plan_token: "old".to_string(),
        aws_account_id: account.to_string(),
        plan_file: PathBuf::from("/tmp/cloudsaw-plan-old"),
        planned_principal_arn: "arn:aws:iam::111122223333:role/X".into(),
        policy_variant: PolicyVariant::SecurityAudit,
        no_changes: false,
        changes: Vec::new(),
        created_at: chrono::Utc::now(),
    });
    // A second plan supersedes it:
    plans::insert(plans::PlanEntry {
        plan_token: "new".to_string(),
        aws_account_id: account.to_string(),
        plan_file: PathBuf::from("/tmp/cloudsaw-plan-new"),
        planned_principal_arn: "arn:aws:iam::111122223333:role/X".into(),
        policy_variant: PolicyVariant::SecurityAudit,
        no_changes: false,
        changes: Vec::new(),
        created_at: chrono::Utc::now(),
    });

    // Applying the old token must fail.
    let err = plans::consume(account, "old").unwrap_err();
    assert!(matches!(err, TerraformError::PlanTokenExpired));

    // The new token still works exactly once.
    let entry = plans::consume(account, "new").unwrap();
    assert_eq!(entry.plan_token, "new");
    let err = plans::consume(account, "new").unwrap_err();
    assert!(matches!(err, TerraformError::PlanTokenInvalid));
}

// --- error-code mapping (defends the IPC surface) ------------------------

#[test]
fn terraform_errors_map_to_stable_app_error_codes() {
    let cases = [
        (TerraformError::NotBundled, "terraform_not_bundled"),
        (
            TerraformError::IntegrityFailed,
            "terraform_integrity_failed",
        ),
        (TerraformError::InitFailed, "terraform_init_failed"),
        (TerraformError::PlanFailed, "terraform_plan_failed"),
        (TerraformError::ApplyFailed, "terraform_apply_failed"),
        (
            TerraformError::PlanTokenInvalid,
            "terraform_plan_token_invalid",
        ),
        (
            TerraformError::PlanTokenExpired,
            "terraform_plan_token_expired",
        ),
        (
            TerraformError::IdentityUnresolvable,
            "terraform_identity_unresolvable",
        ),
        (
            TerraformError::TrustVerificationFailed,
            "terraform_trust_verification_failed",
        ),
    ];
    for (err, expected) in cases {
        let mapped: AppError = err.into();
        assert_eq!(mapped.code(), expected);
    }
}

// --- Trust-policy verification ------------------------------------------

#[test]
fn verify_trust_policy_round_trip_against_module_output_shape() {
    // The trust_policy_json output is `jsonencode(...)` of a single-statement
    // policy. Mirror that exactly so we know our verifier accepts the shape
    // Terraform actually produces.
    let policy = serde_json::json!({
        "Version": "2012-10-17",
        "Statement": [
            {
                "Effect": "Allow",
                "Principal": {
                    "AWS": "arn:aws:iam::111122223333:role/CICDDeployer"
                },
                "Action": "sts:AssumeRole",
                "Condition": {
                    "StringEquals": { "sts:ExternalId": "abcdef0123456789" }
                }
            }
        ]
    })
    .to_string();
    runner::verify_trust_policy_principal(&policy, "arn:aws:iam::111122223333:role/CICDDeployer")
        .expect("the actual Terraform output shape must round-trip");
}

#[test]
fn verify_trust_policy_rejects_added_statement() {
    // Multiple statements means the role's trust policy was modified
    // out-of-band (or the module was tampered with). Refuse rather than
    // accept the surprising extra statement.
    let policy = serde_json::json!({
        "Version": "2012-10-17",
        "Statement": [
            {
                "Effect": "Allow",
                "Principal": { "AWS": "arn:aws:iam::111122223333:role/X" },
                "Action": "sts:AssumeRole"
            },
            {
                "Effect": "Allow",
                "Principal": { "AWS": "*" },
                "Action": "sts:AssumeRole"
            }
        ]
    })
    .to_string();
    let err = runner::verify_trust_policy_principal(&policy, "arn:aws:iam::111122223333:role/X")
        .unwrap_err();
    assert!(matches!(err, TerraformError::TrustVerificationFailed));
}

// --- Schema check (defense for credential-bearing columns) ---------------

#[test]
fn accounts_table_after_migration_0004_has_no_credential_columns() {
    let _sb = Sandbox::new("schema-0004");
    accounts_storage::insert(&seed_account("dev", "dev-profile", "111122223333")).unwrap();
    let acc = accounts::get_account("111122223333").unwrap();
    // The columns added by 0004 don't surface on `Account` directly — they
    // live in the row. Round-tripping records:
    tf_storage::ensure_external_id("111122223333").unwrap();
    tf_storage::record_provisioned(
        "111122223333",
        "arn:aws:iam::111122223333:role/CloudSawScannerRole",
        PolicyVariant::ReadOnlyAccess,
    )
    .unwrap();

    // After provisioning, the account row reflects role_provisioned = true.
    let acc_after = accounts::get_account("111122223333").unwrap();
    assert!(acc_after.role_provisioned);
    assert_eq!(acc_after.label, acc.label);

    // Confirm the schema doesn't have credential-bearing columns added by
    // migration 0004 (mirror of the Contract 04 schema test, applied to the
    // post-0004 table).
    let conn = rusqlite::Connection::open(
        std::env::var("CLOUDSAW_DATA_DIR_OVERRIDE")
            .map(|d| PathBuf::from(d).join("db").join("cloudsaw.db"))
            .unwrap(),
    )
    .unwrap();
    let mut stmt = conn
        .prepare("SELECT name FROM pragma_table_info('accounts')")
        .unwrap();
    let cols: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let forbidden = [
        "access_key",
        "aws_access_key_id",
        "secret_key",
        "aws_secret_access_key",
        "session_token",
        "aws_session_token",
        "password",
    ];
    for col in &cols {
        let lower = col.to_ascii_lowercase();
        for bad in &forbidden {
            assert!(
                !lower.contains(bad),
                "post-0004 schema must not contain credential column {col}"
            );
        }
    }
    // The new tracking columns must exist.
    let must_have = [
        "external_id",
        "policy_variant",
        "last_provisioning_error",
        "scanner_role_arn",
    ];
    for name in must_have {
        assert!(
            cols.iter().any(|c| c == name),
            "migration 0004 must add column {name}"
        );
    }
}
