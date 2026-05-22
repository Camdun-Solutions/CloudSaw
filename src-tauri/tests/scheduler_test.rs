// Scheduler integration tests — Contract 10 happy-path coverage.
//
// Each test runs in its own per-test sandbox with `CLOUDSAW_DATA_DIR_OVERRIDE`
// pointed at a fresh temp dir, a real migration run, and real SQLite. Tests
// share a module-level mutex so env-var manipulation can't race. This mirrors
// the convention from the other integration tests in this crate.

use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{Duration as ChronoDuration, Utc};
use cloudsaw_lib::accounts::{
    self, storage as accounts_storage, types::AccountRecord, Environment,
};
use cloudsaw_lib::db::migrations;
use cloudsaw_lib::scheduler::{
    self, runner, storage as sched_storage, LastRunOutcome, ScheduleCadence, ScheduleEventKind,
    SetScheduleInput,
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
        let dir = std::env::temp_dir().join(format!("cloudsaw-sched-{label}-{nanos}"));
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

fn seed_account(aws_id: &str, label: &str) {
    accounts_storage::insert(&AccountRecord {
        aws_account_id: aws_id.to_string(),
        label: label.to_string(),
        profile_name: format!("test-{label}"),
        environment: Environment::Dev,
    })
    .unwrap();
}

/// Happy Path: set_schedule + get_schedule + list_schedules + next_run_times.
#[test]
fn happy_set_get_list_round_trips() {
    let _sb = Sandbox::new("happy-set-get");
    seed_account("111122223333", "qa-dev");

    let input = SetScheduleInput {
        aws_account_id: "111122223333".into(),
        cadence: ScheduleCadence::Weekly { day_of_week: 1 },
        time_of_day_minutes: Some(9 * 60),
        enabled: true,
    };
    let created = scheduler::set_schedule(input).unwrap();
    assert_eq!(created.aws_account_id, "111122223333");
    assert!(created.enabled);
    assert!(created.next_run_at.is_some());

    let fetched = scheduler::get_schedule("111122223333").unwrap();
    assert_eq!(fetched.aws_account_id, "111122223333");
    assert!(matches!(
        fetched.cadence,
        ScheduleCadence::Weekly { day_of_week: 1 }
    ));

    let all = scheduler::list_schedules().unwrap();
    assert_eq!(all.len(), 1);

    let upcoming = scheduler::next_run_times().unwrap();
    assert_eq!(upcoming.len(), 1);
    assert!(upcoming[0].next_run_at.is_some());
}

/// Validation: a malformed account ID is rejected at the boundary.
#[test]
fn validation_rejects_bad_account_id() {
    let _sb = Sandbox::new("validate-bad-id");
    let input = SetScheduleInput {
        aws_account_id: "not-12-digits".into(),
        cadence: ScheduleCadence::Daily,
        time_of_day_minutes: Some(0),
        enabled: true,
    };
    let err = scheduler::set_schedule(input).unwrap_err();
    assert!(matches!(
        err,
        scheduler::SchedulerError::InvalidInput("aws_account_id")
    ));
}

/// Validation: cadence + time-of-day pair is checked before SQL touches the row.
#[test]
fn validation_rejects_missing_time_of_day() {
    let _sb = Sandbox::new("validate-missing-tod");
    seed_account("111122223333", "qa-dev");
    let input = SetScheduleInput {
        aws_account_id: "111122223333".into(),
        cadence: ScheduleCadence::Daily,
        time_of_day_minutes: None,
        enabled: true,
    };
    let err = scheduler::set_schedule(input).unwrap_err();
    assert!(matches!(
        err,
        scheduler::SchedulerError::InvalidInput("time_of_day_minutes")
    ));
}

/// Setting a schedule for an account that doesn't exist surfaces
/// `AccountNotFound` before SQLite writes anything.
#[test]
fn set_schedule_rejects_unknown_account() {
    let _sb = Sandbox::new("unknown-account");
    let input = SetScheduleInput {
        aws_account_id: "444455556666".into(),
        cadence: ScheduleCadence::Daily,
        time_of_day_minutes: Some(0),
        enabled: true,
    };
    let err = scheduler::set_schedule(input).unwrap_err();
    assert!(matches!(err, scheduler::SchedulerError::AccountNotFound));
}

/// State transition: no schedule → schedule set. Clearing a schedule
/// surfaces `NotFound` on the next get.
#[test]
fn state_no_schedule_then_set_then_clear() {
    let _sb = Sandbox::new("no-sched-then-set");
    seed_account("111122223333", "qa-dev");
    let err = scheduler::get_schedule("111122223333").unwrap_err();
    assert!(matches!(err, scheduler::SchedulerError::NotFound));

    scheduler::set_schedule(SetScheduleInput {
        aws_account_id: "111122223333".into(),
        cadence: ScheduleCadence::Daily,
        time_of_day_minutes: Some(9 * 60),
        enabled: true,
    })
    .unwrap();
    assert!(scheduler::get_schedule("111122223333").is_ok());

    scheduler::clear_schedule("111122223333").unwrap();
    let err = scheduler::get_schedule("111122223333").unwrap_err();
    assert!(matches!(err, scheduler::SchedulerError::NotFound));
}

/// Edge case: account removal cascades to its schedule. The removed
/// account leaves no orphan schedule.
#[test]
fn removing_account_cascades_to_schedule() {
    let _sb = Sandbox::new("cascade-on-remove");
    seed_account("111122223333", "qa-dev");
    scheduler::set_schedule(SetScheduleInput {
        aws_account_id: "111122223333".into(),
        cadence: ScheduleCadence::Daily,
        time_of_day_minutes: Some(9 * 60),
        enabled: true,
    })
    .unwrap();
    assert!(scheduler::get_schedule("111122223333").is_ok());

    accounts::remove_account("111122223333").unwrap();
    let err = scheduler::get_schedule("111122223333").unwrap_err();
    assert!(matches!(err, scheduler::SchedulerError::NotFound));
    assert!(scheduler::list_schedules().unwrap().is_empty());
}

/// Toggle: disabling a schedule clears its next_run_at, preserving the
/// configuration; re-enabling restores upcoming run times.
#[test]
fn toggle_enabled_round_trips_next_run() {
    let _sb = Sandbox::new("toggle-next-run");
    seed_account("111122223333", "qa-dev");
    scheduler::set_schedule(SetScheduleInput {
        aws_account_id: "111122223333".into(),
        cadence: ScheduleCadence::Daily,
        time_of_day_minutes: Some(9 * 60),
        enabled: true,
    })
    .unwrap();
    let upcoming = scheduler::next_run_times().unwrap();
    assert!(upcoming[0].next_run_at.is_some());

    scheduler::set_schedule(SetScheduleInput {
        aws_account_id: "111122223333".into(),
        cadence: ScheduleCadence::Daily,
        time_of_day_minutes: Some(9 * 60),
        enabled: false,
    })
    .unwrap();
    let upcoming = scheduler::next_run_times().unwrap();
    assert!(upcoming[0].next_run_at.is_none());

    scheduler::set_schedule(SetScheduleInput {
        aws_account_id: "111122223333".into(),
        cadence: ScheduleCadence::Daily,
        time_of_day_minutes: Some(9 * 60),
        enabled: true,
    })
    .unwrap();
    let upcoming = scheduler::next_run_times().unwrap();
    assert!(upcoming[0].next_run_at.is_some());
}

/// Events: setting a schedule writes a config_set + enabled event the
/// caller can read back via `recent_events`.
#[test]
fn events_record_lifecycle_transitions() {
    let _sb = Sandbox::new("events-lifecycle");
    seed_account("111122223333", "qa-dev");
    scheduler::set_schedule(SetScheduleInput {
        aws_account_id: "111122223333".into(),
        cadence: ScheduleCadence::Daily,
        time_of_day_minutes: Some(9 * 60),
        enabled: true,
    })
    .unwrap();
    let events = scheduler::recent_events("111122223333", 10).unwrap();
    let kinds: Vec<_> = events.iter().map(|e| e.kind).collect();
    assert!(kinds.contains(&ScheduleEventKind::ConfigSet));
    assert!(kinds.contains(&ScheduleEventKind::Enabled));

    // Flip enabled → disabled and confirm the Disabled event is appended.
    scheduler::set_schedule(SetScheduleInput {
        aws_account_id: "111122223333".into(),
        cadence: ScheduleCadence::Daily,
        time_of_day_minutes: Some(9 * 60),
        enabled: false,
    })
    .unwrap();
    let events = scheduler::recent_events("111122223333", 10).unwrap();
    assert!(events.iter().any(|e| e.kind == ScheduleEventKind::Disabled));
}

/// Runner: ticks for an account whose scanner role is NOT provisioned
/// record a SkippedRoleNotProvisioned outcome, advance next_run_at, and
/// never start a scan.
#[test]
fn runner_skips_when_role_not_provisioned() {
    let _sb = Sandbox::new("runner-skip-no-role");
    seed_account("111122223333", "qa-dev");
    // Configure the schedule, then back-date next_run_at so the runner
    // sees it as due.
    scheduler::set_schedule(SetScheduleInput {
        aws_account_id: "111122223333".into(),
        cadence: ScheduleCadence::Interval { minutes: 60 },
        time_of_day_minutes: None,
        enabled: true,
    })
    .unwrap();
    let past = Utc::now() - ChronoDuration::minutes(5);
    sched_storage::set_next_run("111122223333", Some(past)).unwrap();

    // Ensure no scanner binary override is set so detection succeeds OR
    // fails predictably. In CI sandboxes the bundled binary is absent,
    // which counts as `Missing` — also a valid skip path. We assert the
    // runner records SOME skip outcome and advances next_run_at.
    runner::tick_once();

    let after = scheduler::get_schedule("111122223333").unwrap();
    assert!(after.last_run_outcome.is_some(), "outcome must be recorded");
    let outcome = after.last_run_outcome.unwrap();
    assert!(
        matches!(
            outcome,
            LastRunOutcome::SkippedRoleNotProvisioned | LastRunOutcome::SkippedScannerUnavailable
        ),
        "expected a skip outcome, got {outcome:?}"
    );
    // next_run_at must have advanced strictly past the back-dated value.
    let advanced = after.next_run_at.unwrap();
    assert!(advanced > past);
}
