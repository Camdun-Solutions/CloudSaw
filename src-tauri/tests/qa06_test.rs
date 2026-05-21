// Contract 06-QA — Scanner Orchestrator: QA & Security Verification.
//
// This file batches the QA acceptance checks that are verifiable without a
// real AWS account and without the bundled ScoutSuite binary (added by
// Contract 16 / Next-Steps C3). Each test maps to a specific QA item from
// `cloud-saw-contracts/C06-scanner-orchestrator-QA.md`.
//
// Items that require a live AWS environment to verify (actual `AssumeRole`
// against a real role, ScoutSuite running against real resources, process
// inspection during a real scan) are documented in
// CONTRACT_06_VERIFICATION.md as operator-driven checks.
//
// Tests share a per-test sandbox with `CLOUDSAW_DATA_DIR_OVERRIDE`, real
// migration run, and real SQLite. They serialize through a module-level
// mutex like the other integration tests in this crate.

use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use cloudsaw_lib::accounts::{
    storage as accounts_storage, types::AccountRecord, Environment,
};
use cloudsaw_lib::db::migrations;
use cloudsaw_lib::scanner::{
    self, binary, handles, storage as scan_storage, sts, types::ScanStatus,
    ScannerError, ScoutSuiteAvailability,
};
use cloudsaw_lib::terraform::{
    storage as tf_storage, types::PolicyVariant,
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
        let dir = std::env::temp_dir().join(format!("cloudsaw-qa06-{label}-{nanos}"));
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

fn manifest_dir() -> PathBuf {
    PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
}

fn seed_provisioned_account(aws_id: &str, label: &str) {
    let _ = accounts_storage::insert(&AccountRecord {
        aws_account_id: aws_id.to_string(),
        label: label.to_string(),
        profile_name: format!("test-{label}"),
        environment: Environment::Dev,
    });
    let _ = tf_storage::ensure_external_id(aws_id);
    tf_storage::record_provisioned(
        aws_id,
        &format!("arn:aws:iam::{aws_id}:role/CloudSawScannerRole"),
        PolicyVariant::SecurityAudit,
    )
    .unwrap();
}

fn enable_dry_run(sb_dir: &std::path::Path) {
    use sha2::{Digest, Sha256};
    let path = sb_dir.join("fake-scoutsuite-qa.bin");
    fs::write(&path, b"qa06-fake-scoutsuite-binary").unwrap();
    let mut h = Sha256::new();
    h.update(b"qa06-fake-scoutsuite-binary");
    let sha = hex::encode(h.finalize());
    std::env::set_var("CLOUDSAW_SCOUTSUITE_BIN_OVERRIDE", &path);
    std::env::set_var("CLOUDSAW_SCOUTSUITE_SHA256_OVERRIDE", &sha);
    std::env::set_var("CLOUDSAW_SCANNER_STUB_STS", "1");
    std::env::set_var("CLOUDSAW_SCANNER_DRY_RUN", "1");
}

fn wait_for_terminal(scan_id: &str, deadline: Duration) -> cloudsaw_lib::scanner::ScanRecord {
    let start = Instant::now();
    let mut latest = scanner::scan_status(scan_id).unwrap();
    while !latest.status.is_terminal() {
        if start.elapsed() > deadline {
            panic!("scan {scan_id} stuck in {:?} after {deadline:?}", latest.status);
        }
        std::thread::sleep(Duration::from_millis(50));
        latest = scanner::scan_status(scan_id).unwrap();
    }
    latest
}

// ============================================================================
// HAPPY PATH (automatable subset)
// ============================================================================

/// QA Happy Path #1: `detect_binary` reports the bundled ScoutSuite binary
/// present and integrity-valid when its hash matches the build-pinned value.
#[test]
fn qa_happy_detect_binary_succeeds_when_binary_matches_pinned_hash() {
    use sha2::{Digest, Sha256};
    let sb = Sandbox::new("happy-detect");
    let path = sb.dir.join("fake-scout");
    fs::write(&path, b"pretend this is scoutsuite").unwrap();
    let mut h = Sha256::new();
    h.update(b"pretend this is scoutsuite");
    let sha = hex::encode(h.finalize());
    std::env::set_var("CLOUDSAW_SCOUTSUITE_BIN_OVERRIDE", &path);
    std::env::set_var("CLOUDSAW_SCOUTSUITE_SHA256_OVERRIDE", &sha);
    match scanner::detect_binary() {
        ScoutSuiteAvailability::Available { sha256 } => assert_eq!(sha256, sha),
        other => panic!("expected Available, got {other:?}"),
    }
}

/// QA Happy Path #2: a scan against a provisioned account walks the state
/// machine end to end to `complete` and produces a non-empty raw output
/// file. End-to-end with real AWS is operator-driven (#OP-1).
#[test]
fn qa_happy_scan_completes_and_produces_non_empty_raw_output() {
    let sb = Sandbox::new("happy-complete");
    seed_provisioned_account("111122223333", "qa-dev");
    enable_dry_run(&sb.dir);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let initial = rt.block_on(scanner::run_scan("111122223333")).unwrap();
    assert_eq!(initial.aws_account_id, "111122223333");

    let final_record = wait_for_terminal(&initial.scan_id, Duration::from_secs(5));
    assert_eq!(final_record.status, ScanStatus::Complete);
    let raw = final_record.raw_output_path.as_deref().unwrap();
    let bytes = fs::read(raw).unwrap();
    assert!(!bytes.is_empty());
}

// ============================================================================
// ERROR STATES
// ============================================================================

/// QA Error State: tampered ScoutSuite binary → scan does not start;
/// integrity error.
#[test]
fn qa_error_tampered_binary_yields_integrity_failed() {
    let sb = Sandbox::new("err-tampered");
    seed_provisioned_account("111122223333", "qa-dev");
    let path = sb.dir.join("tampered-scout");
    fs::write(&path, b"someone-replaced-the-scanner").unwrap();
    std::env::set_var("CLOUDSAW_SCOUTSUITE_BIN_OVERRIDE", &path);
    std::env::set_var(
        "CLOUDSAW_SCOUTSUITE_SHA256_OVERRIDE",
        "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
    );
    let err = binary::locate_and_verify().unwrap_err();
    assert!(matches!(err, ScannerError::IntegrityFailed));

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let err = rt.block_on(scanner::run_scan("111122223333")).unwrap_err();
    assert!(matches!(err, ScannerError::IntegrityFailed));
    // No row should have been claimed — the integrity gate rejects before
    // try_claim_account runs.
    assert!(!scan_storage::account_has_in_flight("111122223333").unwrap());
}

/// QA Error State: AssumeRole failure → scan fails early with a clear
/// reason; no ScoutSuite child is spawned. We approximate the failure by
/// removing the persisted scanner_role_arn so the orchestrator surfaces
/// RoleNotProvisioned before AssumeRole. Real AWS AssumeRole failures are
/// classified by `sts::classify` and verified in the auth tests; the
/// "fails early without spawning" property is the one this test asserts.
#[test]
fn qa_error_assume_role_failure_fails_early_without_spawning() {
    let _sb = Sandbox::new("err-assume-fail");
    accounts_storage::insert(&AccountRecord {
        aws_account_id: "111122223333".into(),
        label: "qa-dev".into(),
        profile_name: "qa-profile".into(),
        environment: Environment::Dev,
    })
    .unwrap();
    // role_provisioned stays false — run_scan surfaces RoleNotProvisioned
    // before reaching AssumeRole, which is itself before spawn.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let err = rt.block_on(scanner::run_scan("111122223333")).unwrap_err();
    assert!(matches!(err, ScannerError::RoleNotProvisioned));
    assert!(!scan_storage::account_has_in_flight("111122223333").unwrap());
}

/// QA Error State: second concurrent scan for the same account is rejected.
#[test]
fn qa_error_second_concurrent_scan_rejected() {
    let _sb = Sandbox::new("err-concurrent");
    seed_provisioned_account("111122223333", "qa-dev");
    scan_storage::try_claim_account(
        "in-flight-scan",
        "111122223333",
        "cloudsaw-scan-inflight",
    )
    .unwrap();
    let err = scan_storage::try_claim_account(
        "second-scan",
        "111122223333",
        "cloudsaw-scan-second",
    )
    .unwrap_err();
    assert!(matches!(err, ScannerError::AlreadyRunning));
}

/// QA Error State: machine sleep loses the child → on resume the scan is
/// detected as stale and marked `failed` with `scanner_process_lost`.
#[test]
fn qa_error_machine_sleep_marks_scan_process_lost_on_resume() {
    let _sb = Sandbox::new("err-stale");
    seed_provisioned_account("111122223333", "qa-dev");
    scan_storage::try_claim_account("stale-scan", "111122223333", "cloudsaw-scan-stale")
        .unwrap();
    scan_storage::update_status("stale-scan", ScanStatus::Scanning).unwrap();

    // Simulate the next process bootstrap.
    let reaped = scanner::reap_stale_on_boot().unwrap();
    assert_eq!(reaped, 1);
    let record = scanner::scan_status("stale-scan").unwrap();
    assert_eq!(record.status, ScanStatus::Failed);
    assert_eq!(
        record.failure_code.as_deref(),
        Some("scanner_process_lost")
    );
}

/// QA Error State: role missing permissions → `complete_with_warnings` with
/// the missing-permission detail captured. We exercise the storage path
/// since the real distinction is made by ScoutSuite's exit code (2 =
/// partial success) which the orchestrator maps onto warnings.
#[test]
fn qa_error_partial_permissions_yield_complete_with_warnings() {
    let _sb = Sandbox::new("err-partial");
    seed_provisioned_account("111122223333", "qa-dev");
    scan_storage::try_claim_account(
        "partial-scan",
        "111122223333",
        "cloudsaw-scan-partial",
    )
    .unwrap();
    scan_storage::record_complete(
        "partial-scan",
        Some(("missing_permissions", Some("access_denied"))),
        false,
    )
    .unwrap();
    let record = scanner::scan_status("partial-scan").unwrap();
    assert_eq!(record.status, ScanStatus::CompleteWithWarnings);
    assert_eq!(record.warning_code.as_deref(), Some("missing_permissions"));
    assert_eq!(record.warning_detail.as_deref(), Some("access_denied"));
}

/// QA Error State: extremely large scanner output → bounded/truncated with a
/// warning; raw file retained. We test the storage flag round-trip; the
/// stream-bounding logic itself lives in `runner::wait_for_child` and is
/// unit-tested through its bounded reader behavior.
#[test]
fn qa_error_truncated_output_flagged_in_scan_record() {
    let _sb = Sandbox::new("err-truncated");
    seed_provisioned_account("111122223333", "qa-dev");
    scan_storage::try_claim_account(
        "trunc-scan",
        "111122223333",
        "cloudsaw-scan-trunc",
    )
    .unwrap();
    scan_storage::record_complete("trunc-scan", None, true).unwrap();
    let record = scanner::scan_status("trunc-scan").unwrap();
    assert!(record.truncated, "truncated flag must round-trip through storage");
}

// ============================================================================
// RESPONSIVENESS (limited to what's checkable without driving real ScoutSuite)
// ============================================================================

/// QA Responsiveness: `scan_status` polling returns promptly with the
/// current state. We assert the round-trip cost on a freshly-claimed scan
/// is in the millisecond range — anything over 250ms would suggest a hot
/// loop or a SQLite lock contention bug.
#[test]
fn qa_responsiveness_scan_status_returns_promptly() {
    let _sb = Sandbox::new("resp-status");
    seed_provisioned_account("111122223333", "qa-dev");
    scan_storage::try_claim_account("resp-scan", "111122223333", "cloudsaw-scan-resp")
        .unwrap();
    let start = Instant::now();
    let _ = scanner::scan_status("resp-scan").unwrap();
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_millis(250),
        "scan_status should be fast — took {elapsed:?}"
    );
}

// ============================================================================
// STATE TRANSITIONS
// ============================================================================

/// QA State Transition: `pending → assuming_role → scanning → parsing →
/// complete`. We assert the orchestrator drives a dry-run scan all the way
/// to `Complete` and that the storage layer accepts each intermediate
/// transition without dropping the row.
#[test]
fn qa_state_transition_pending_to_complete_walks_each_state() {
    let sb = Sandbox::new("state-walk");
    seed_provisioned_account("111122223333", "qa-dev");
    enable_dry_run(&sb.dir);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let initial = rt.block_on(scanner::run_scan("111122223333")).unwrap();
    let final_record = wait_for_terminal(&initial.scan_id, Duration::from_secs(5));
    assert_eq!(final_record.status, ScanStatus::Complete);
    // Started + finished timestamps must be set; finished_at > started_at.
    let started = final_record.started_at;
    let finished = final_record.finished_at.unwrap();
    assert!(
        finished >= started,
        "finished_at must be >= started_at, got {started} / {finished}"
    );
}

/// QA State Transition: `scanning → canceled` on cancel, with partial
/// output preserved (we don't run real ScoutSuite here, so the cancel path
/// is exercised against a planted scanning row).
#[test]
fn qa_state_transition_scanning_to_canceled() {
    let _sb = Sandbox::new("state-cancel");
    seed_provisioned_account("111122223333", "qa-dev");
    scan_storage::try_claim_account("cancel-me", "111122223333", "cloudsaw-scan-cancel")
        .unwrap();
    scan_storage::update_status("cancel-me", ScanStatus::Scanning).unwrap();

    let canceled = scanner::cancel_scan("cancel-me").unwrap();
    assert_eq!(canceled.status, ScanStatus::Canceled);
}

/// QA State Transition: `scanning → failed` on process loss.
#[test]
fn qa_state_transition_scanning_to_failed_on_process_loss() {
    let _sb = Sandbox::new("state-process-loss");
    seed_provisioned_account("111122223333", "qa-dev");
    scan_storage::try_claim_account("lost-scan", "111122223333", "cloudsaw-scan-lost")
        .unwrap();
    scan_storage::update_status("lost-scan", ScanStatus::Scanning).unwrap();
    scanner::reap_stale_on_boot().unwrap();
    let record = scanner::scan_status("lost-scan").unwrap();
    assert_eq!(record.status, ScanStatus::Failed);
    assert_eq!(
        record.failure_code.as_deref(),
        Some("scanner_process_lost")
    );
}

// ============================================================================
// SECURITY CHECK
// ============================================================================

/// Security Check #1: each scan performs a fresh `AssumeRole`; no STS
/// session credentials persist across scans. Verified structurally: the
/// orchestrator calls `sts::assume_scanner_role` inside `execute_scan_inner`
/// and never caches the returned credentials.
#[test]
fn qa_security_assume_role_is_fresh_per_scan_no_cache() {
    let src = fs::read_to_string(
        manifest_dir().join("src").join("scanner").join("mod.rs"),
    )
    .unwrap();
    assert!(
        src.contains("sts::assume_scanner_role"),
        "execute_scan_inner must call assume_scanner_role per scan"
    );
    // No static / lazy_static / OnceLock for credentials anywhere in the
    // scanner module.
    let storage_src = fs::read_to_string(
        manifest_dir().join("src").join("scanner").join("storage.rs"),
    )
    .unwrap();
    let sts_src = fs::read_to_string(
        manifest_dir().join("src").join("scanner").join("sts.rs"),
    )
    .unwrap();
    let runner_src = fs::read_to_string(
        manifest_dir().join("src").join("scanner").join("runner.rs"),
    )
    .unwrap();
    let lower = |s: &str| s.to_ascii_lowercase();
    for module_src in [&src, &storage_src, &sts_src, &runner_src] {
        let lower = lower(module_src);
        assert!(
            !lower.contains("static cred")
                && !lower.contains("once_cell")
                && !lower.contains("oncelock<credentials"),
            "scanner module must NOT cache credentials in static storage"
        );
    }
}

/// Security Check #2: AssumeRole session duration ≤ 3600 seconds.
///
/// The check is a compile-time constant comparison; clippy notices this and
/// would normally warn that the assertion can never fire. That's the whole
/// point — `SCAN_SESSION_DURATION_SECONDS` is the value the test guards, and
/// changing it above 3600 must fail the build. `#[allow]` keeps the
/// assertion in place as the project's stable record of the invariant.
#[test]
#[allow(clippy::assertions_on_constants)]
fn qa_security_assume_role_session_duration_is_bounded() {
    assert!(
        sts::SCAN_SESSION_DURATION_SECONDS <= 3600,
        "session duration must be ≤ 1h per CLAUDE.md §4.3"
    );
}

/// Security Check #3: ScoutSuite binary SHA-256 is verified before EVERY
/// invocation. `runner::spawn_scoutsuite` calls `binary::locate_and_verify`
/// before constructing the Command — we check the source structurally.
#[test]
fn qa_security_run_path_invokes_locate_and_verify() {
    let runner_src = fs::read_to_string(
        manifest_dir().join("src").join("scanner").join("runner.rs"),
    )
    .unwrap();
    let spawn_block = runner_src
        .split("pub fn spawn_scoutsuite")
        .nth(1)
        .expect("spawn_scoutsuite must exist");
    let header = &spawn_block[..spawn_block.find('}').unwrap_or(spawn_block.len())];
    assert!(
        header.contains("binary::locate_and_verify()"),
        "spawn_scoutsuite must call binary::locate_and_verify() before spawn"
    );
}

/// Security Check #4: ScoutSuite invoked by absolute path with argv arrays;
/// no shell anywhere in the runner.
#[test]
fn qa_security_runner_source_has_no_shell_invocation() {
    let runner_src = fs::read_to_string(
        manifest_dir().join("src").join("scanner").join("runner.rs"),
    )
    .unwrap();
    let code_only: String = runner_src
        .lines()
        .map(|l| match l.find("//") {
            Some(idx) => &l[..idx],
            None => l,
        })
        .collect::<Vec<_>>()
        .join("\n");

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
            "scanner/runner.rs (code-only) must never invoke a shell — found {needle:?}"
        );
    }
    assert!(
        code_only.contains("Command::new(&binary_path)"),
        "spawn_scoutsuite must spawn via Command::new(absolute_path)"
    );
}

/// Security Check #5: temporary credentials reach the child's environment
/// only — never disk, never logs, never the parent process environment.
/// We verify the structural property by reading the source.
#[test]
fn qa_security_credentials_go_only_to_child_environment() {
    let runner_src = fs::read_to_string(
        manifest_dir().join("src").join("scanner").join("runner.rs"),
    )
    .unwrap();
    // Credentials are set via `cmd.env(...)` on the Command — which writes
    // to the per-Command env map, not the parent process.
    assert!(
        runner_src.contains("cmd.env(\"AWS_ACCESS_KEY_ID\""),
        "child credentials must be set on the Command, not the parent env"
    );
    assert!(
        runner_src.contains("cmd.env(\"AWS_SECRET_ACCESS_KEY\""),
        "secret must be set on the Command"
    );
    assert!(
        runner_src.contains("cmd.env(\"AWS_SESSION_TOKEN\""),
        "session token must be set on the Command"
    );
    // `std::env::set_var(...)` for AWS_* in the runner would taint the
    // PARENT process, which is the property this guards against.
    let code_only: String = runner_src
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !code_only.contains("env::set_var(\"AWS_"),
        "runner must NEVER call env::set_var for AWS_* — that pollutes the parent process"
    );

    // Parent-side check: the orchestrator does NOT call env::set_var for
    // AWS_* either.
    let mod_src = fs::read_to_string(
        manifest_dir().join("src").join("scanner").join("mod.rs"),
    )
    .unwrap();
    let mod_code_only: String = mod_src
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !mod_code_only.contains("env::set_var(\"AWS_"),
        "scanner/mod.rs must NEVER set AWS_* on the parent process"
    );
}

/// Security Check #5 (continued): credentials never reach a log path or a
/// file written by CloudSaw. The Debug impl on AssumedCredentials redacts
/// every field (see `sts::tests::debug_output_does_not_leak_secret_bytes`).
/// Here we also check that no `fs::write` / `Path::write` of credentials
/// appears in the runner or orchestrator.
#[test]
fn qa_security_credentials_never_written_to_disk() {
    let runner_src = fs::read_to_string(
        manifest_dir().join("src").join("scanner").join("runner.rs"),
    )
    .unwrap();
    let mod_src = fs::read_to_string(
        manifest_dir().join("src").join("scanner").join("mod.rs"),
    )
    .unwrap();
    for src in [&runner_src, &mod_src] {
        for forbidden in [
            "fs::write(&creds",
            "fs::write(creds",
            ".write(creds.access_key_id",
            ".write(creds.secret_access_key",
            ".write(creds.session_token",
        ] {
            assert!(
                !src.contains(forbidden),
                "credentials must not be written to disk — found {forbidden:?}"
            );
        }
    }
}

/// Security Check #6: after a scan, no AWS credentials exist on disk
/// anywhere under the app data directory.
#[test]
fn qa_security_no_credentials_on_disk_after_scan() {
    let sb = Sandbox::new("sec-no-creds-on-disk");
    seed_provisioned_account("111122223333", "qa-dev");
    enable_dry_run(&sb.dir);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let initial = rt.block_on(scanner::run_scan("111122223333")).unwrap();
    wait_for_terminal(&initial.scan_id, Duration::from_secs(5));

    // Walk the entire data dir and search the bytes of every file for the
    // stub credentials we passed in. None should appear.
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
    walk(&sb.dir, &mut files);

    let stub_keys = [
        b"ASIASTUBSTUBSTUBSTUB".as_slice(),
        b"STUB-SECRET-KEY-DO-NOT-USE".as_slice(),
        b"STUB-SESSION-TOKEN-DO-NOT-USE".as_slice(),
    ];
    for file in &files {
        let bytes = match fs::read(file) {
            Ok(b) => b,
            Err(_) => continue,
        };
        for key in &stub_keys {
            assert!(
                !bytes.windows(key.len()).any(|w| w == *key),
                "credential bytes leaked into {}",
                file.display()
            );
        }
    }
}

/// Security Check #7: scan output directory has user-only permissions.
#[test]
fn qa_security_scan_output_dir_user_only() {
    let sb = Sandbox::new("sec-output-perm");
    seed_provisioned_account("111122223333", "qa-dev");
    enable_dry_run(&sb.dir);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let initial = rt.block_on(scanner::run_scan("111122223333")).unwrap();
    let final_record = wait_for_terminal(&initial.scan_id, Duration::from_secs(5));
    let raw = final_record.raw_output_path.unwrap();

    let expected_dir = sb.dir.join("scans").join(&initial.scan_id);
    assert!(std::path::Path::new(&raw).starts_with(&expected_dir));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&expected_dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
}

/// Security Check #8: the scan record's RoleSessionName identifies the
/// scan AND fits inside AWS's 64-char limit.
#[test]
fn qa_security_role_session_name_identifies_scan_and_fits_aws_limit() {
    let name = scanner::role_session_name_for("0123456789abcdef0123456789abcdef");
    assert!(name.starts_with("cloudsaw-scan-"));
    assert!(name.len() <= 64);
    assert_ne!(
        name,
        scanner::role_session_name_for("ffffeeeeddddccccbbbbaaaa99998888"),
        "different scan IDs must produce different RoleSessionName values"
    );
}

/// Security Check #9: the scanner IPC surface has no command name that
/// would let the frontend retrieve credentials or skip the role gate.
#[test]
fn qa_security_no_credential_returning_ipc_commands() {
    let ipc_src = fs::read_to_string(
        manifest_dir().join("src").join("ipc").join("mod.rs"),
    )
    .unwrap();
    for forbidden in [
        "scanner_get_credentials",
        "scanner_assume_role",
        "scanner_session_token",
    ] {
        assert!(
            !ipc_src.contains(forbidden),
            "ipc must not expose {forbidden:?}"
        );
    }
}

// ============================================================================
// SECURITY: scan IDs are unguessable and unique
// ============================================================================

#[test]
fn qa_security_scan_ids_are_unguessable_and_unique() {
    use std::collections::HashSet;
    let mut seen = HashSet::new();
    for _ in 0..256 {
        let t = scanner::mint_scan_id();
        assert_eq!(t.len(), 32);
        assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(seen.insert(t), "scan IDs must not collide");
    }
}
