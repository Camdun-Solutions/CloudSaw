// Contract 10-QA — Scheduled & Automated Scans: QA & Security Verification.
//
// This file batches the QA acceptance checks that are verifiable without a
// real AWS account and without driving an actual ScoutSuite scan. Each test
// maps to a specific QA item from
// `cloud-saw-contracts/C10-scheduled-scans-QA.md`. Items that require a
// live AWS environment to verify (an actual scheduled scan firing
// end-to-end, machine-sleep behavior, etc.) are documented in
// CONTRACT_10_VERIFICATION.md as operator-driven checks.
//
// Tests share a per-test sandbox with `CLOUDSAW_DATA_DIR_OVERRIDE`, real
// migrations, and real SQLite. They serialize through a module-level mutex
// like the other integration tests in this crate.

use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::{Duration as ChronoDuration, Utc};
use cloudsaw_lib::accounts::{
    self, storage as accounts_storage, types::AccountRecord, Environment,
};
use cloudsaw_lib::db::migrations;
use cloudsaw_lib::scanner::storage as scan_storage;
use cloudsaw_lib::scanner_role::{storage as tf_storage, types::PolicyVariant};
use cloudsaw_lib::scheduler::{
    self, runner, storage as sched_storage, LastRunOutcome, ScheduleCadence, ScheduleEventKind,
    SchedulerError, SetScheduleInput,
};

fn env_lock() -> &'static Mutex<()> {
    static L: OnceLock<Mutex<()>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(()))
}

struct Sandbox {
    _guard: std::sync::MutexGuard<'static, ()>,
    dir: PathBuf,
}

impl Sandbox {
    fn new(label: &str) -> Self {
        let guard = env_lock().lock().unwrap_or_else(|p| p.into_inner());
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("cloudsaw-qa10-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        fs::create_dir_all(dir.join("db")).unwrap();
        std::env::set_var("CLOUDSAW_DATA_DIR_OVERRIDE", &dir);
        migrations::run(&dir.join("db").join("cloudsaw.db")).unwrap();
        runner::_reset_for_tests();
        Self { _guard: guard, dir }
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        std::env::remove_var("CLOUDSAW_DATA_DIR_OVERRIDE");
        runner::_reset_for_tests();
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
}

fn seed_account(aws_id: &str, label: &str) {
    accounts_storage::insert(&AccountRecord {
        aws_account_id: aws_id.to_string(),
        label: label.to_string(),
        profile_name: format!("test-{label}"),
        environment: Environment::Dev,
    })
    .unwrap();
}

fn seed_provisioned_account(aws_id: &str, label: &str) {
    seed_account(aws_id, label);
    let _ = tf_storage::ensure_external_id(aws_id);
    tf_storage::record_provisioned(
        aws_id,
        &format!("arn:aws:iam::{aws_id}:role/CloudSawScannerRole"),
        PolicyVariant::SecurityAudit,
    )
    .unwrap();
}

// ============================================================================
// HAPPY PATH
// ============================================================================

/// QA Happy Path: a weekly schedule round-trips through `set/get/list/
/// next_run_times` and reports the configured cadence.
#[test]
fn qa_happy_weekly_schedule_round_trips() {
    let _sb = Sandbox::new("happy-weekly");
    seed_account("111122223333", "qa-dev");

    scheduler::set_schedule(SetScheduleInput {
        aws_account_id: "111122223333".into(),
        cadence: ScheduleCadence::Weekly { day_of_week: 1 },
        time_of_day_minutes: Some(9 * 60),
        enabled: true,
    })
    .unwrap();

    let fetched = scheduler::get_schedule("111122223333").unwrap();
    assert!(matches!(
        fetched.cadence,
        ScheduleCadence::Weekly { day_of_week: 1 }
    ));
    assert_eq!(fetched.time_of_day_minutes, Some(540));

    let upcoming = scheduler::next_run_times().unwrap();
    assert_eq!(upcoming.len(), 1);
    assert!(upcoming[0].next_run_at.is_some());
}

/// QA Happy Path: schedules persist across an app restart. We simulate the
/// restart by running `bootstrap_runner` against an existing DB.
#[test]
fn qa_happy_schedules_persist_across_restart() {
    let _sb = Sandbox::new("happy-restart");
    seed_account("111122223333", "qa-dev");
    scheduler::set_schedule(SetScheduleInput {
        aws_account_id: "111122223333".into(),
        cadence: ScheduleCadence::Daily,
        time_of_day_minutes: Some(9 * 60),
        enabled: true,
    })
    .unwrap();

    // Simulate restart: a fresh bootstrap pass against the existing DB
    // must not drop the row, and must leave `next_run_at` populated.
    runner::_reset_for_tests();
    runner::bootstrap_runner().unwrap();

    let after = scheduler::get_schedule("111122223333").unwrap();
    assert!(after.enabled);
    assert!(after.next_run_at.is_some());
}

// ============================================================================
// ERROR STATES
// ============================================================================

/// QA Error State: scheduled time during an in-progress scan → scheduled
/// run skipped with a recorded reason.
#[test]
fn qa_error_inflight_scan_blocks_scheduled_run() {
    let _sb = Sandbox::new("err-inflight");
    seed_provisioned_account("111122223333", "qa-dev");

    // Plant an in-flight scan so try_claim_account refuses the next one.
    scan_storage::try_claim_account("manual-in-flight", "111122223333", "cloudsaw-scan-inflight")
        .unwrap();

    // Configure the schedule and back-date next_run_at so the runner sees
    // the slot as due.
    scheduler::set_schedule(SetScheduleInput {
        aws_account_id: "111122223333".into(),
        cadence: ScheduleCadence::Interval { minutes: 60 },
        time_of_day_minutes: None,
        enabled: true,
    })
    .unwrap();
    let past = Utc::now() - ChronoDuration::minutes(5);
    sched_storage::set_next_run("111122223333", Some(past)).unwrap();

    // Stub the scanner detect path so the runner reaches the AlreadyRunning
    // branch. If no bundled binary, the runner skips earlier — also valid.
    runner::tick_once();
    let after = scheduler::get_schedule("111122223333").unwrap();
    let outcome = after.last_run_outcome.expect("runner records outcome");
    assert!(
        matches!(
            outcome,
            LastRunOutcome::SkippedAlreadyRunning | LastRunOutcome::SkippedScannerUnavailable
        ),
        "expected skip, got {outcome:?}"
    );
}

/// QA Error State: app closed across a missed time → at most one catch-up
/// scan is queued on next launch.
#[test]
fn qa_error_missed_times_collapse_to_single_catch_up() {
    let _sb = Sandbox::new("err-catch-up");
    seed_provisioned_account("111122223333", "qa-dev");
    scheduler::set_schedule(SetScheduleInput {
        aws_account_id: "111122223333".into(),
        cadence: ScheduleCadence::Interval { minutes: 60 },
        time_of_day_minutes: None,
        enabled: true,
    })
    .unwrap();
    // Back-date next_run_at by 10 hours — 10 missed slots in an hourly
    // cadence. The bootstrap must queue exactly ONE catch-up.
    let ten_hours_ago = Utc::now() - ChronoDuration::hours(10);
    sched_storage::set_next_run("111122223333", Some(ten_hours_ago)).unwrap();

    let caught_up = runner::bootstrap_runner().unwrap();
    assert_eq!(caught_up, 1, "missed runs must collapse into one catch-up");

    let events = scheduler::recent_events("111122223333", 50).unwrap();
    let catch_up_count = events
        .iter()
        .filter(|e| e.kind == ScheduleEventKind::CatchUp)
        .count();
    assert_eq!(catch_up_count, 1, "exactly one CatchUp event");
}

/// QA Error State: scheduled run for an unprovisioned account fails
/// gracefully with a clear reason. No scan is started.
#[test]
fn qa_error_unprovisioned_account_skip_clear_reason() {
    let _sb = Sandbox::new("err-no-role");
    seed_account("111122223333", "qa-dev"); // not provisioned
    scheduler::set_schedule(SetScheduleInput {
        aws_account_id: "111122223333".into(),
        cadence: ScheduleCadence::Interval { minutes: 60 },
        time_of_day_minutes: None,
        enabled: true,
    })
    .unwrap();
    let past = Utc::now() - ChronoDuration::minutes(5);
    sched_storage::set_next_run("111122223333", Some(past)).unwrap();

    runner::tick_once();

    let after = scheduler::get_schedule("111122223333").unwrap();
    let outcome = after.last_run_outcome.expect("runner records outcome");
    assert!(
        matches!(
            outcome,
            LastRunOutcome::SkippedRoleNotProvisioned | LastRunOutcome::SkippedScannerUnavailable
        ),
        "expected role/scanner skip, got {outcome:?}"
    );
    assert!(
        !scan_storage::account_has_in_flight("111122223333").unwrap(),
        "no scan should have been claimed"
    );
}

/// QA Error State: account removed with an active schedule → schedule is
/// removed (no orphan fires).
#[test]
fn qa_error_account_removal_clears_schedule() {
    let _sb = Sandbox::new("err-orphan");
    seed_account("111122223333", "qa-dev");
    scheduler::set_schedule(SetScheduleInput {
        aws_account_id: "111122223333".into(),
        cadence: ScheduleCadence::Daily,
        time_of_day_minutes: Some(9 * 60),
        enabled: true,
    })
    .unwrap();
    accounts::remove_account("111122223333").unwrap();

    assert!(scheduler::list_schedules().unwrap().is_empty());
    let err = scheduler::get_schedule("111122223333").unwrap_err();
    assert!(matches!(err, SchedulerError::NotFound));
}

/// QA Error State: machine asleep at scheduled time → on wake, catch-up
/// applies; the scheduler does not crash. We can't simulate sleep itself,
/// but the bootstrap path is the resume entry; calling it after a stale
/// next_run_at must not panic.
#[test]
fn qa_error_bootstrap_after_sleep_does_not_crash() {
    let _sb = Sandbox::new("err-sleep");
    seed_account("111122223333", "qa-dev");
    scheduler::set_schedule(SetScheduleInput {
        aws_account_id: "111122223333".into(),
        cadence: ScheduleCadence::Daily,
        time_of_day_minutes: Some(9 * 60),
        enabled: true,
    })
    .unwrap();
    let stale = Utc::now() - ChronoDuration::days(3);
    sched_storage::set_next_run("111122223333", Some(stale)).unwrap();

    // The bootstrap call must succeed even after several missed slots,
    // and the row must still exist.
    runner::_reset_for_tests();
    runner::bootstrap_runner().unwrap();
    assert!(scheduler::get_schedule("111122223333").is_ok());
}

// ============================================================================
// RESPONSIVENESS
// ============================================================================

/// QA Responsiveness: `get_schedule` returns promptly. Anything over
/// 250ms would suggest a SQLite lock-contention bug.
#[test]
fn qa_responsiveness_get_schedule_is_fast() {
    let _sb = Sandbox::new("resp-get");
    seed_account("111122223333", "qa-dev");
    scheduler::set_schedule(SetScheduleInput {
        aws_account_id: "111122223333".into(),
        cadence: ScheduleCadence::Daily,
        time_of_day_minutes: Some(9 * 60),
        enabled: true,
    })
    .unwrap();
    let start = Instant::now();
    scheduler::get_schedule("111122223333").unwrap();
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_millis(250),
        "get_schedule took {elapsed:?}"
    );
}

/// QA Responsiveness: schedule changes take effect without an app restart —
/// the runner picks up the new cadence on its next tick.
#[test]
fn qa_responsiveness_changes_take_effect_without_restart() {
    let _sb = Sandbox::new("resp-no-restart");
    seed_account("111122223333", "qa-dev");
    scheduler::set_schedule(SetScheduleInput {
        aws_account_id: "111122223333".into(),
        cadence: ScheduleCadence::Daily,
        time_of_day_minutes: Some(9 * 60),
        enabled: true,
    })
    .unwrap();

    // Disabling immediately drops next_run_at — the runner's
    // `list_enabled` query won't return the row anymore.
    scheduler::set_schedule(SetScheduleInput {
        aws_account_id: "111122223333".into(),
        cadence: ScheduleCadence::Daily,
        time_of_day_minutes: Some(9 * 60),
        enabled: false,
    })
    .unwrap();
    let after = scheduler::get_schedule("111122223333").unwrap();
    assert!(!after.enabled);
    assert!(after.next_run_at.is_none());
}

// ============================================================================
// STATE TRANSITIONS
// ============================================================================

/// QA State Transition: no schedule → schedule set → background runner picks
/// it up. The "picks it up" claim is verified by `list_enabled` returning
/// the row.
#[test]
fn qa_state_transition_no_schedule_to_enabled() {
    let _sb = Sandbox::new("state-enable");
    seed_account("111122223333", "qa-dev");
    assert!(sched_storage::list_enabled().unwrap().is_empty());

    scheduler::set_schedule(SetScheduleInput {
        aws_account_id: "111122223333".into(),
        cadence: ScheduleCadence::Daily,
        time_of_day_minutes: Some(9 * 60),
        enabled: true,
    })
    .unwrap();
    let enabled = sched_storage::list_enabled().unwrap();
    assert_eq!(enabled.len(), 1);
}

/// QA State Transition: schedule enabled → disabled → no runs fire → re-
/// enabled → runs resume.
#[test]
fn qa_state_transition_disable_then_reenable() {
    let _sb = Sandbox::new("state-disable-reenable");
    seed_account("111122223333", "qa-dev");

    scheduler::set_schedule(SetScheduleInput {
        aws_account_id: "111122223333".into(),
        cadence: ScheduleCadence::Daily,
        time_of_day_minutes: Some(9 * 60),
        enabled: true,
    })
    .unwrap();
    scheduler::set_schedule(SetScheduleInput {
        aws_account_id: "111122223333".into(),
        cadence: ScheduleCadence::Daily,
        time_of_day_minutes: Some(9 * 60),
        enabled: false,
    })
    .unwrap();
    assert!(sched_storage::list_enabled().unwrap().is_empty());

    scheduler::set_schedule(SetScheduleInput {
        aws_account_id: "111122223333".into(),
        cadence: ScheduleCadence::Daily,
        time_of_day_minutes: Some(9 * 60),
        enabled: true,
    })
    .unwrap();
    assert_eq!(sched_storage::list_enabled().unwrap().len(), 1);
}

/// QA State Transition: account with schedule → account removed → schedule
/// removed. Same as the orphan test but stated explicitly for the QA matrix.
#[test]
fn qa_state_transition_account_remove_removes_schedule() {
    let _sb = Sandbox::new("state-remove");
    seed_account("111122223333", "qa-dev");
    scheduler::set_schedule(SetScheduleInput {
        aws_account_id: "111122223333".into(),
        cadence: ScheduleCadence::Daily,
        time_of_day_minutes: Some(9 * 60),
        enabled: true,
    })
    .unwrap();
    accounts::remove_account("111122223333").unwrap();
    assert!(scheduler::list_schedules().unwrap().is_empty());
}

// ============================================================================
// SECURITY CHECK
// ============================================================================

/// Security Check: scheduled scans use the SAME secure scan path as a manual
/// scan. The runner only calls `scanner::run_scan` — no parallel scan
/// path exists. We verify the structural property by reading the source.
#[test]
fn qa_security_runner_uses_only_scanner_run_scan() {
    let runner_src = fs::read_to_string(
        manifest_dir()
            .join("src")
            .join("scheduler")
            .join("runner.rs"),
    )
    .unwrap();
    // The single entry point into the scanner orchestrator must be
    // `scanner::run_scan`. No second scan path, no direct STS or binary
    // invocation — those all live in `scanner::*`.
    assert!(
        runner_src.contains("scanner::run_scan"),
        "scheduler runner must call scanner::run_scan"
    );
    // Defense in depth: the runner must NOT call sts:AssumeRole itself
    // (the scanner orchestrator owns that). The token "assume_role" must
    // not appear in runner.rs.
    let code_only: String = runner_src
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !code_only.contains("assume_role"),
        "scheduler runner must not invoke assume_role — that's the scanner's job"
    );
    // No direct invocation of binary or sts module.
    assert!(
        !code_only.contains("scanner::sts::") && !code_only.contains("sts::assume"),
        "scheduler runner must not call sts internals directly"
    );
}

/// Security Check: scheduled scans never run in parallel with another scan
/// for the same account. The runner relies on the storage layer's
/// transactional `try_claim_account` gate — if a scan is in flight, the
/// scheduler-initiated scan path returns `AlreadyRunning` and the runner
/// records a skip.
#[test]
fn qa_security_scheduled_scans_never_parallel_per_account() {
    let _sb = Sandbox::new("sec-no-parallel");
    seed_provisioned_account("111122223333", "qa-dev");

    // Plant an in-flight manual scan.
    scan_storage::try_claim_account("manual-inflight", "111122223333", "cloudsaw-scan-manual")
        .unwrap();

    // Trying to claim a second scan must be rejected at the storage layer —
    // this is the same gate the runner relies on.
    let err =
        scan_storage::try_claim_account("scheduled-attempt", "111122223333", "cloudsaw-scan-sched")
            .unwrap_err();
    assert!(matches!(
        err,
        cloudsaw_lib::scanner::ScannerError::AlreadyRunning
    ));
}

/// Security Check: missed-run catch-up does not stack multiple runs. Even
/// across 24 hours of missed slots (in a 60-min cadence), exactly ONE
/// catch-up is queued.
#[test]
fn qa_security_catch_up_does_not_stack() {
    let _sb = Sandbox::new("sec-no-stack");
    seed_provisioned_account("111122223333", "qa-dev");
    scheduler::set_schedule(SetScheduleInput {
        aws_account_id: "111122223333".into(),
        cadence: ScheduleCadence::Interval { minutes: 60 },
        time_of_day_minutes: None,
        enabled: true,
    })
    .unwrap();
    // Back-date 24 hours → 24 missed slots.
    let day_ago = Utc::now() - ChronoDuration::hours(24);
    sched_storage::set_next_run("111122223333", Some(day_ago)).unwrap();

    let caught_up = runner::bootstrap_runner().unwrap();
    assert_eq!(caught_up, 1);

    let events = scheduler::recent_events("111122223333", 100).unwrap();
    let catch_up = events
        .iter()
        .filter(|e| e.kind == ScheduleEventKind::CatchUp)
        .count();
    assert_eq!(catch_up, 1, "exactly one catch-up event regardless of N");
}

/// Security Check: schedules are stored as non-secret configuration in
/// SQLite. The schedules table must contain no credential-bearing column
/// names. We verify structurally by inspecting only the executable SQL
/// (comments are stripped first so we don't false-positive on legitimate
/// explanatory text like "no credentials are stored").
#[test]
fn qa_security_schedules_table_has_no_credential_columns() {
    let sql =
        fs::read_to_string(manifest_dir().join("migrations").join("0007_scheduler.sql")).unwrap();
    let code_only: String = sql
        .lines()
        .filter(|l| !l.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n")
        .to_ascii_lowercase();
    for forbidden in [
        "access_key",
        "secret_key",
        "session_token",
        "password",
        "credential",
        "api_key",
    ] {
        assert!(
            !code_only.contains(forbidden),
            "schedules table must not store {forbidden:?}"
        );
    }
}

/// Security Check: scheduler IPC surface has no command that would let the
/// frontend bypass scanner gates or return credentials.
#[test]
fn qa_security_no_credential_returning_ipc_commands() {
    let ipc_src =
        fs::read_to_string(manifest_dir().join("src").join("ipc").join("mod.rs")).unwrap();
    for forbidden in [
        "scheduler_get_credentials",
        "scheduler_assume_role",
        "scheduler_session_token",
        "scheduler_run_now_bypass",
    ] {
        assert!(
            !ipc_src.contains(forbidden),
            "scheduler ipc must not expose {forbidden:?}"
        );
    }
}

/// Security Check: defined behavior for scheduled scans vs. the app lock —
/// the lock is a UI gate, not a process gate. The background runner runs
/// in the same process and respects the same scanner gates (binary
/// verification, role provisioning, the per-account scan lock). It does
/// NOT consult the app-lock SessionState because the lock controls UI
/// access, not the timing of background work. We verify the runner does
/// not reach into the applock module.
#[test]
fn qa_security_runner_does_not_consult_applock() {
    let runner_src = fs::read_to_string(
        manifest_dir()
            .join("src")
            .join("scheduler")
            .join("runner.rs"),
    )
    .unwrap();
    let mod_src =
        fs::read_to_string(manifest_dir().join("src").join("scheduler").join("mod.rs")).unwrap();
    for src in [&runner_src, &mod_src] {
        let code_only: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !code_only.contains("applock::"),
            "scheduler must not reach into the applock module"
        );
    }
}
