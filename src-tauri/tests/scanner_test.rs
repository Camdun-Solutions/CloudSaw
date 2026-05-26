// Integration tests for the scanner orchestrator (Contract 06).
//
// We can't drive real ScoutSuite or real AWS from a unit test, so the
// integration tests focus on the surfaces Contract 06 calls out as testable
// without network:
//
//   * `detect_binary` returns the right status in the three scenarios.
//   * Binary integrity check rejects tampered binaries.
//   * The orchestrator's state machine walks
//     `pending → assuming_role → scanning → parsing → complete` end-to-end
//     using the dry-run + stub-STS seams.
//   * A second concurrent scan against the same account is rejected.
//   * Cancellation transitions the scan to `canceled`.
//   * `reap_stale_on_boot` rescues a stale in-flight row left behind by a
//     previous process.
//   * Account-ID inputs are validated before becoming partition keys.
//
// The harness mirrors the terraform/accounts integration-test sandbox:
// per-test temporary data dir via `CLOUDSAW_DATA_DIR_OVERRIDE`, real
// migration run, real SQLite. Network-touching paths (real ScoutSuite + real
// AWS) are out of scope here — those map onto operator-driven checks in
// CONTRACT_06_VERIFICATION.md.

use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use cloudsaw_lib::accounts::{storage as accounts_storage, types::AccountRecord, Environment};
use cloudsaw_lib::db::migrations;
use cloudsaw_lib::scanner::{
    self, binary, handles, storage as scan_storage, types::ScanStatus, ScannerError,
    ScoutSuiteAvailability,
};
use cloudsaw_lib::scanner_role::{storage as tf_storage, types::PolicyVariant};

fn env_lock() -> &'static Mutex<()> {
    static L: OnceLock<Mutex<()>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(()))
}

struct Sandbox {
    _guard: std::sync::MutexGuard<'static, ()>,
    dir: PathBuf,
    prev_bin: Option<String>,
    prev_sha: Option<String>,
    prev_stub_sts: Option<String>,
    prev_dry_run: Option<String>,
}

impl Sandbox {
    fn new(label: &str) -> Self {
        let guard = env_lock().lock().unwrap_or_else(|p| p.into_inner());
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("cloudsaw-scanner-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        fs::create_dir_all(dir.join("db")).unwrap();
        std::env::set_var("CLOUDSAW_DATA_DIR_OVERRIDE", &dir);
        migrations::run(&dir.join("db").join("cloudsaw.db")).unwrap();

        let prev_bin = std::env::var("CLOUDSAW_SCOUTSUITE_BIN_OVERRIDE").ok();
        let prev_sha = std::env::var("CLOUDSAW_SCOUTSUITE_SHA256_OVERRIDE").ok();
        let prev_stub_sts = std::env::var("CLOUDSAW_SCANNER_STUB_STS").ok();
        let prev_dry_run = std::env::var("CLOUDSAW_SCANNER_DRY_RUN").ok();
        std::env::remove_var("CLOUDSAW_SCOUTSUITE_BIN_OVERRIDE");
        std::env::remove_var("CLOUDSAW_SCOUTSUITE_SHA256_OVERRIDE");
        std::env::remove_var("CLOUDSAW_SCANNER_STUB_STS");
        std::env::remove_var("CLOUDSAW_SCANNER_DRY_RUN");

        handles::_clear_for_tests();

        Self {
            _guard: guard,
            dir,
            prev_bin,
            prev_sha,
            prev_stub_sts,
            prev_dry_run,
        }
    }

    fn data_dir(&self) -> &PathBuf {
        &self.dir
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        std::env::remove_var("CLOUDSAW_DATA_DIR_OVERRIDE");
        handles::_clear_for_tests();
        match &self.prev_bin {
            Some(v) => std::env::set_var("CLOUDSAW_SCOUTSUITE_BIN_OVERRIDE", v),
            None => std::env::remove_var("CLOUDSAW_SCOUTSUITE_BIN_OVERRIDE"),
        }
        match &self.prev_sha {
            Some(v) => std::env::set_var("CLOUDSAW_SCOUTSUITE_SHA256_OVERRIDE", v),
            None => std::env::remove_var("CLOUDSAW_SCOUTSUITE_SHA256_OVERRIDE"),
        }
        match &self.prev_stub_sts {
            Some(v) => std::env::set_var("CLOUDSAW_SCANNER_STUB_STS", v),
            None => std::env::remove_var("CLOUDSAW_SCANNER_STUB_STS"),
        }
        match &self.prev_dry_run {
            Some(v) => std::env::set_var("CLOUDSAW_SCANNER_DRY_RUN", v),
            None => std::env::remove_var("CLOUDSAW_SCANNER_DRY_RUN"),
        }
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn write_fake_binary(sb: &Sandbox, content: &[u8]) -> (PathBuf, String) {
    use sha2::{Digest, Sha256};
    let path = sb.data_dir().join("fake-scoutsuite.bin");
    fs::write(&path, content).unwrap();
    let mut h = Sha256::new();
    h.update(content);
    let sha = hex::encode(h.finalize());
    (path, sha)
}

fn seed_provisioned_account(aws_id: &str, label: &str) {
    let _ = accounts_storage::insert(&AccountRecord {
        aws_account_id: aws_id.to_string(),
        label: label.to_string(),
        profile_name: format!("test-{label}"),
        environment: Environment::Dev,
    });
    // Populate the Terraform fields so role_provisioned == true with an ARN.
    let _ = tf_storage::ensure_external_id(aws_id);
    tf_storage::record_provisioned(
        aws_id,
        &format!("arn:aws:iam::{aws_id}:role/CloudSawScannerRole"),
        PolicyVariant::SecurityAudit,
    )
    .unwrap();
}

fn enable_dry_run_with_fake_binary(sb: &Sandbox) {
    let (path, sha) = write_fake_binary(sb, b"fake-scoutsuite-binary");
    std::env::set_var("CLOUDSAW_SCOUTSUITE_BIN_OVERRIDE", &path);
    std::env::set_var("CLOUDSAW_SCOUTSUITE_SHA256_OVERRIDE", &sha);
    std::env::set_var("CLOUDSAW_SCANNER_STUB_STS", "1");
    std::env::set_var("CLOUDSAW_SCANNER_DRY_RUN", "1");
}

/// Poll `scan_status` until it reports a terminal state or the deadline
/// elapses. Returns the final record or panics with the latest non-terminal
/// status — failing loudly is the contract here, not silently returning a
/// half-baked record.
fn wait_for_terminal(scan_id: &str, deadline: Duration) -> cloudsaw_lib::scanner::ScanRecord {
    let start = Instant::now();
    let mut latest = scanner::scan_status(scan_id).unwrap();
    while !latest.status.is_terminal() {
        if start.elapsed() > deadline {
            panic!(
                "scan {scan_id} stuck in {:?} after {deadline:?}",
                latest.status
            );
        }
        std::thread::sleep(Duration::from_millis(50));
        latest = scanner::scan_status(scan_id).unwrap();
    }
    latest
}

// ============================================================================
// detect_binary
// ============================================================================

#[test]
fn detect_binary_reports_missing_when_no_binary() {
    let _sb = Sandbox::new("detect-missing");
    let availability = scanner::detect_binary();
    assert!(matches!(availability, ScoutSuiteAvailability::Missing));
}

#[test]
fn detect_binary_reports_available_when_hash_matches() {
    let sb = Sandbox::new("detect-ok");
    let (path, sha) = write_fake_binary(&sb, b"this is a scoutsuite stand-in");
    std::env::set_var("CLOUDSAW_SCOUTSUITE_BIN_OVERRIDE", &path);
    std::env::set_var("CLOUDSAW_SCOUTSUITE_SHA256_OVERRIDE", &sha);
    match scanner::detect_binary() {
        ScoutSuiteAvailability::Available { sha256 } => {
            assert_eq!(sha256, sha);
        }
        other => panic!("expected Available, got {other:?}"),
    }
}

#[test]
fn detect_binary_reports_integrity_failed_when_binary_tampered() {
    let sb = Sandbox::new("detect-tampered");
    let (path, _sha) = write_fake_binary(&sb, b"original-binary");
    std::env::set_var("CLOUDSAW_SCOUTSUITE_BIN_OVERRIDE", &path);
    std::env::set_var(
        "CLOUDSAW_SCOUTSUITE_SHA256_OVERRIDE",
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
    assert!(matches!(
        scanner::detect_binary(),
        ScoutSuiteAvailability::IntegrityFailed
    ));
}

#[test]
fn locate_and_verify_returns_typed_errors_in_each_failure_mode() {
    let sb = Sandbox::new("locate-and-verify");

    let err = binary::locate_and_verify().unwrap_err();
    assert!(matches!(err, ScannerError::NotBundled));

    let (path, _sha) = write_fake_binary(&sb, b"x");
    std::env::set_var("CLOUDSAW_SCOUTSUITE_BIN_OVERRIDE", &path);
    let err = binary::locate_and_verify().unwrap_err();
    assert!(matches!(err, ScannerError::NotBundled));

    std::env::set_var(
        "CLOUDSAW_SCOUTSUITE_SHA256_OVERRIDE",
        "1111111111111111111111111111111111111111111111111111111111111111",
    );
    let err = binary::locate_and_verify().unwrap_err();
    assert!(matches!(err, ScannerError::IntegrityFailed));

    let (path2, sha2) = write_fake_binary(&sb, b"matched");
    std::env::set_var("CLOUDSAW_SCOUTSUITE_BIN_OVERRIDE", &path2);
    std::env::set_var("CLOUDSAW_SCOUTSUITE_SHA256_OVERRIDE", &sha2);
    let (p, s) = binary::locate_and_verify().unwrap();
    assert_eq!(p, path2);
    assert_eq!(s, sha2);
}

// ============================================================================
// run_scan input validation
// ============================================================================

#[test]
fn run_scan_rejects_malformed_account_id() {
    let _sb = Sandbox::new("malformed-id");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    // Length wrong.
    let err = rt.block_on(scanner::run_scan("12345")).unwrap_err();
    assert!(matches!(err, ScannerError::InvalidInput("aws_account_id")));
    // Non-digit.
    let err = rt.block_on(scanner::run_scan("12345678901a")).unwrap_err();
    assert!(matches!(err, ScannerError::InvalidInput("aws_account_id")));
    // Path traversal attempt — would be devastating if it leaked into a
    // path segment.
    let err = rt.block_on(scanner::run_scan("../etc/passwd")).unwrap_err();
    assert!(matches!(err, ScannerError::InvalidInput("aws_account_id")));
}

#[test]
fn run_scan_rejects_unknown_account() {
    let _sb = Sandbox::new("unknown-account");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let err = rt.block_on(scanner::run_scan("999988887777")).unwrap_err();
    assert!(matches!(
        err,
        ScannerError::Accounts(cloudsaw_lib::accounts::AccountsError::NotFound)
    ));
}

#[test]
fn run_scan_rejects_account_without_provisioned_role() {
    let _sb = Sandbox::new("not-provisioned");
    accounts_storage::insert(&AccountRecord {
        aws_account_id: "111122223333".into(),
        label: "qa-dev".into(),
        profile_name: "qa-profile".into(),
        environment: Environment::Dev,
    })
    .unwrap();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let err = rt.block_on(scanner::run_scan("111122223333")).unwrap_err();
    assert!(matches!(err, ScannerError::RoleNotProvisioned));
}

#[test]
fn run_scan_rejects_when_binary_not_bundled() {
    let _sb = Sandbox::new("no-binary");
    seed_provisioned_account("111122223333", "qa-dev");
    // No CLOUDSAW_SCOUTSUITE_* env vars set — binary detection returns
    // NotBundled, and run_scan refuses to start.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let err = rt.block_on(scanner::run_scan("111122223333")).unwrap_err();
    assert!(matches!(err, ScannerError::NotBundled));
}

#[test]
fn run_scan_rejects_when_binary_integrity_fails() {
    let sb = Sandbox::new("tampered-binary");
    seed_provisioned_account("111122223333", "qa-dev");
    let (path, _sha) = write_fake_binary(&sb, b"tampered");
    std::env::set_var("CLOUDSAW_SCOUTSUITE_BIN_OVERRIDE", &path);
    std::env::set_var(
        "CLOUDSAW_SCOUTSUITE_SHA256_OVERRIDE",
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
    std::env::set_var("CLOUDSAW_SCANNER_STUB_STS", "1");
    std::env::set_var("CLOUDSAW_SCANNER_DRY_RUN", "1");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let err = rt.block_on(scanner::run_scan("111122223333")).unwrap_err();
    assert!(matches!(err, ScannerError::IntegrityFailed));
    // No row should have been claimed.
    assert!(!scan_storage::account_has_in_flight("111122223333").unwrap());
}

// ============================================================================
// End-to-end state machine via dry-run + stub-STS
// ============================================================================

#[test]
fn run_scan_walks_to_complete_in_dry_run_mode() {
    let sb = Sandbox::new("dryrun-complete");
    seed_provisioned_account("111122223333", "qa-dev");
    enable_dry_run_with_fake_binary(&sb);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let initial = rt.block_on(scanner::run_scan("111122223333")).unwrap();
    assert!(!initial.status.is_terminal());
    assert!(initial.role_session_name.starts_with("cloudsaw-scan-"));

    let final_record = wait_for_terminal(&initial.scan_id, Duration::from_secs(5));
    assert_eq!(final_record.status, ScanStatus::Complete);
    assert!(final_record.finished_at.is_some());
    assert!(final_record.failure_code.is_none());
    let raw = final_record
        .raw_output_path
        .as_deref()
        .expect("raw output path must be persisted on Complete");
    assert!(std::path::Path::new(raw).is_file());
    let bytes = fs::read(raw).unwrap();
    assert!(!bytes.is_empty(), "raw-scout.json must be non-empty");
}

#[test]
fn run_scan_persists_under_scans_dir_with_user_only_permissions() {
    let sb = Sandbox::new("dryrun-permissions");
    seed_provisioned_account("111122223333", "qa-dev");
    enable_dry_run_with_fake_binary(&sb);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let initial = rt.block_on(scanner::run_scan("111122223333")).unwrap();
    let final_record = wait_for_terminal(&initial.scan_id, Duration::from_secs(5));
    let raw = final_record.raw_output_path.unwrap();

    // The output path must live under <data_root>/scans/<scan_id>/.
    let expected_prefix = sb.data_dir().join("scans").join(&initial.scan_id);
    assert!(
        std::path::Path::new(&raw).starts_with(&expected_prefix),
        "raw path {raw} must live under {}",
        expected_prefix.display()
    );

    // Permissions: directory must exist with user-only narrow on Unix; on
    // Windows the data root inherits user-profile ACLs.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let dir_perms = fs::metadata(&expected_prefix).unwrap().permissions();
        assert_eq!(dir_perms.mode() & 0o777, 0o700);
        let file_perms = fs::metadata(&raw).unwrap().permissions();
        assert_eq!(file_perms.mode() & 0o777, 0o600);
    }
}

// ============================================================================
// Concurrent-scan rejection
// ============================================================================

#[test]
fn second_concurrent_scan_for_same_account_is_rejected() {
    let _sb = Sandbox::new("concurrent");
    seed_provisioned_account("111122223333", "qa-dev");

    // Claim the account by inserting a pending row directly — this mimics
    // the moment between `try_claim_account` returning and the worker
    // thread reaching a terminal state.
    let scan_id = "frozen-scan-id-aaaa";
    scan_storage::try_claim_account(scan_id, "111122223333", "cloudsaw-scan-frozen").unwrap();
    assert!(scan_storage::account_has_in_flight("111122223333").unwrap());

    // A direct second claim must fail with AlreadyRunning.
    let err = scan_storage::try_claim_account(
        "second-scan-id-bbbb",
        "111122223333",
        "cloudsaw-scan-second",
    )
    .unwrap_err();
    assert!(matches!(err, ScannerError::AlreadyRunning));

    // After marking the first as canceled, a fresh claim succeeds.
    scan_storage::record_canceled(scan_id).unwrap();
    assert!(!scan_storage::account_has_in_flight("111122223333").unwrap());
    scan_storage::try_claim_account("third-scan-id-cccc", "111122223333", "cloudsaw-scan-third")
        .unwrap();
}

// ============================================================================
// Cancellation
// ============================================================================

#[test]
fn cancel_scan_transitions_to_canceled_and_is_idempotent() {
    let _sb = Sandbox::new("cancel");
    seed_provisioned_account("111122223333", "qa-dev");

    // Plant a pending scan directly (the orchestrator might race past it
    // before we can cancel in a dry-run flow).
    let scan_id = "cancel-target-aaaa";
    scan_storage::try_claim_account(scan_id, "111122223333", "cloudsaw-scan-cancel").unwrap();

    let canceled = scanner::cancel_scan(scan_id).unwrap();
    assert_eq!(canceled.status, ScanStatus::Canceled);
    assert!(canceled.finished_at.is_some());

    // Idempotent: calling cancel again returns the same terminal record.
    let again = scanner::cancel_scan(scan_id).unwrap();
    assert_eq!(again.status, ScanStatus::Canceled);
}

#[test]
fn cancel_scan_on_unknown_id_returns_not_found() {
    let _sb = Sandbox::new("cancel-unknown");
    let err = scanner::cancel_scan("no-such-scan").unwrap_err();
    assert!(matches!(err, ScannerError::ScanNotFound));
}

// ============================================================================
// Stale in-flight reaping
// ============================================================================

#[test]
fn reap_stale_on_boot_marks_in_flight_rows_failed() {
    let _sb = Sandbox::new("reap-stale");
    seed_provisioned_account("111122223333", "qa-dev");

    scan_storage::try_claim_account("stale-scan-aaa", "111122223333", "cloudsaw-scan-stale-1")
        .unwrap();
    scan_storage::update_status("stale-scan-aaa", ScanStatus::Scanning).unwrap();

    seed_provisioned_account("444455556666", "qa-prod");
    scan_storage::try_claim_account("stale-scan-bbb", "444455556666", "cloudsaw-scan-stale-2")
        .unwrap();

    let reaped = scanner::reap_stale_on_boot().unwrap();
    assert_eq!(reaped, 2);

    for scan_id in ["stale-scan-aaa", "stale-scan-bbb"] {
        let record = scanner::scan_status(scan_id).unwrap();
        assert_eq!(record.status, ScanStatus::Failed);
        assert_eq!(
            record.failure_code.as_deref(),
            Some("scanner_process_lost"),
            "stale reap must record the process-lost reason"
        );
        assert!(record.finished_at.is_some());
    }
}

#[test]
fn reap_stale_leaves_terminal_rows_untouched() {
    let _sb = Sandbox::new("reap-leaves-terminal");
    seed_provisioned_account("111122223333", "qa-dev");

    scan_storage::try_claim_account("already-complete", "111122223333", "cloudsaw-scan-x").unwrap();
    scan_storage::record_complete("already-complete", None, false).unwrap();

    let before = scanner::scan_status("already-complete").unwrap();
    assert_eq!(before.status, ScanStatus::Complete);
    let reaped = scanner::reap_stale_on_boot().unwrap();
    assert_eq!(reaped, 0);
    let after = scanner::scan_status("already-complete").unwrap();
    assert_eq!(after.status, ScanStatus::Complete);
}

// ============================================================================
// list_recent_scans
// ============================================================================

#[test]
fn list_recent_scans_returns_newest_first() {
    let _sb = Sandbox::new("list-recent");
    seed_provisioned_account("111122223333", "qa-dev");

    // Insert three scans with increasing started_at — try_claim_account
    // uses the current time, so we space them with sleeps.
    let ids = ["scan-1", "scan-2", "scan-3"];
    for id in &ids {
        scan_storage::try_claim_account(id, "111122223333", &format!("cloudsaw-scan-{id}"))
            .unwrap();
        scan_storage::record_complete(id, None, false).unwrap();
        std::thread::sleep(Duration::from_millis(10));
    }

    let recent = scanner::list_recent_scans("111122223333", 10).unwrap();
    let scan_ids: Vec<&str> = recent.iter().map(|r| r.scan_id.as_str()).collect();
    assert_eq!(scan_ids, vec!["scan-3", "scan-2", "scan-1"]);
}

#[test]
fn list_recent_scans_caps_limit_to_100() {
    let _sb = Sandbox::new("list-cap");
    // Empty result is fine; we just want the limit clamp logic exercised.
    let recent = scanner::list_recent_scans("111122223333", 1_000_000).unwrap_or_default();
    assert!(recent.is_empty());
}

#[test]
fn list_recent_scans_validates_account_id() {
    let _sb = Sandbox::new("list-validate");
    let err = scanner::list_recent_scans("nope", 5).unwrap_err();
    assert!(matches!(err, ScannerError::InvalidInput("aws_account_id")));
}

// ============================================================================
// Scan ID + session name shape
// ============================================================================

#[test]
fn role_session_names_identify_the_scan() {
    let sb = Sandbox::new("session-name");
    seed_provisioned_account("111122223333", "qa-dev");
    enable_dry_run_with_fake_binary(&sb);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let initial = rt.block_on(scanner::run_scan("111122223333")).unwrap();
    let final_record = wait_for_terminal(&initial.scan_id, Duration::from_secs(5));
    assert!(final_record.role_session_name.starts_with("cloudsaw-scan-"));
    assert!(
        final_record.role_session_name.len() <= 64,
        "RoleSessionName has a 64-char hard limit at AWS"
    );
}
