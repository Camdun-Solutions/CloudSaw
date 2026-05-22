// Background runner — the loop that wakes on a short cadence, asks the
// storage layer "what's due?", and dispatches scans through the existing
// scanner orchestrator (Contract 06). The orchestrator owns AssumeRole +
// binary verification + the one-scan-per-account gate; this module adds
// only timing.
//
// Lifecycle:
//   * `bootstrap_runner()` is called once at app launch (from `lib::run`).
//     It rounds every persisted `next_run_at` forward to the first slot
//     after `now` so missed-while-closed runs collapse into a single
//     catch-up (Contract 10 §Constraints).
//   * `start_runner()` spawns the poll thread on a `tokio` runtime. The
//     thread is named `cloudsaw-sched` so it's identifiable in process
//     listings.
//   * The runner stops when the process exits — no explicit shutdown is
//     needed. The next launch resumes by reading `schedules` from disk.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration as StdDuration;

use chrono::{DateTime, Utc};

use super::cadence;
use super::error::SchedulerError;
use super::storage;
use super::types::{LastRunOutcome, Schedule, ScheduleEventKind};
use crate::accounts;
use crate::scanner;
use rand_core::{OsRng, RngCore};

/// The runner's poll interval. Short enough that a minute-granularity
/// schedule fires within the same minute; long enough that polling SQLite
/// is not a noticeable cost. Overridable in tests via the
/// `CLOUDSAW_SCHEDULER_POLL_MS` env var.
const DEFAULT_POLL_MS: u64 = 30_000;

/// Module-level "is the runner already running" guard. Set on first
/// `start_runner()`, never cleared. A second call is a no-op so tests and
/// the bootstrap path can both call it without coordinating.
static STARTED: AtomicBool = AtomicBool::new(false);

/// One-time bootstrap on app launch. Collapses missed runs into a single
/// catch-up per account (Contract 10 §Edge Cases: "missed runs do not
/// stack"). Returns the number of catch-up rows queued so the caller can
/// log a count.
pub fn bootstrap_runner() -> Result<usize, SchedulerError> {
    let now = Utc::now();
    let mut caught_up = 0usize;

    let schedules = storage::list_enabled()?;
    for schedule in schedules {
        match schedule.next_run_at {
            None => {
                // No precomputed next run (just-enabled row or pre-bootstrap
                // state). Compute the first one strictly after now.
                let next = cadence::next_after(
                    schedule.cadence,
                    schedule.time_of_day_minutes,
                    now,
                );
                if let Some(n) = next {
                    storage::set_next_run(&schedule.aws_account_id, Some(n))?;
                }
            }
            Some(due) if due <= now => {
                // We missed at least one slot. Record a single CatchUp
                // event so the user sees what happened, then advance the
                // next-run anchor forward without firing here — the
                // poll loop fires on its next tick when it sees the
                // (now-rounded) anchor is also <= now.
                //
                // Crucially, the rounding step uses the missed `due` as
                // the seed and walks forward to the first slot at-or-just-
                // after `now`. That collapses "12 missed daily slots into
                // one fire" rather than 12 fires (the Edge-Case rule).
                let rounded = cadence::round_forward(
                    schedule.cadence,
                    schedule.time_of_day_minutes,
                    due,
                    now,
                );
                // We deliberately want the scheduler to immediately fire
                // ONE catch-up. Set next_run_at to `now` so the next poll
                // sees it due, then advance to `rounded` once it fires.
                // The CatchUp tag on the run record distinguishes it from
                // a normal fire.
                storage::set_next_run(&schedule.aws_account_id, Some(now))?;
                storage::append_event(
                    &mint_event_id(),
                    &schedule.aws_account_id,
                    now,
                    ScheduleEventKind::CatchUp,
                    Some("catch_up_pending"),
                    None,
                )?;
                // Stash the rounded anchor on the row's update timestamp
                // implicitly via the eventual record_run; if catch-up
                // doesn't happen before the next bootstrap (e.g. the
                // catch-up itself was skipped), the round_forward call
                // re-runs and produces the same anchor — idempotent.
                let _ = rounded;
                caught_up += 1;
            }
            Some(_) => { /* future — nothing to do. */ }
        }
    }
    Ok(caught_up)
}

/// Spawn the poll thread once per process. The thread runs on its own
/// `tokio` runtime because the scanner orchestrator's `run_scan` is async.
/// Returns `true` if it spawned the thread, `false` if a runner was already
/// running.
pub fn start_runner() -> bool {
    if STARTED.swap(true, Ordering::SeqCst) {
        return false;
    }
    let poll = poll_interval();
    std::thread::Builder::new()
        .name("cloudsaw-sched".into())
        .spawn(move || run_loop(poll))
        .expect("scheduler runner thread");
    true
}

fn poll_interval() -> StdDuration {
    if let Ok(raw) = std::env::var("CLOUDSAW_SCHEDULER_POLL_MS") {
        if let Ok(parsed) = raw.parse::<u64>() {
            if parsed > 0 {
                return StdDuration::from_millis(parsed);
            }
        }
    }
    StdDuration::from_millis(DEFAULT_POLL_MS)
}

fn run_loop(poll: StdDuration) {
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(_) => return,
    };
    let stop = Arc::new(AtomicBool::new(false));
    rt.block_on(async move {
        loop {
            tick_once();
            if stop.load(Ordering::Relaxed) {
                break;
            }
            tokio::time::sleep(poll).await;
        }
    });
}

/// One poll tick. Exposed so integration tests can drive the runner
/// deterministically without sleeping for the poll interval. Errors are
/// logged-and-swallowed — a transient SQLite hiccup must not crash the
/// background thread.
pub fn tick_once() {
    let now = Utc::now();
    let schedules = match storage::list_enabled() {
        Ok(s) => s,
        Err(_) => return,
    };
    for s in schedules {
        if let Some(next_run_at) = s.next_run_at {
            if next_run_at <= now {
                handle_due(&s, now);
            }
        }
    }
}

/// Decide what to do for a schedule whose `next_run_at` is in the past.
/// One of: fire (start a scan), skip (record reason), or catch-up (single
/// fire after a missed window). Then advance `next_run_at` to the next
/// upcoming slot.
fn handle_due(schedule: &Schedule, now: DateTime<Utc>) {
    let account_id = &schedule.aws_account_id;

    // Detect a pending catch-up: the bootstrap path stamped a
    // ScheduleEventKind::CatchUp row right before resetting next_run_at to
    // `now`. We treat this run as a CatchUp regardless of whether it
    // fires or skips. If the schedule has no catch-up pending the
    // outcome is the normal `Fired` tag.
    let is_catch_up = matches!(
        last_event_kind(account_id),
        Some(ScheduleEventKind::CatchUp),
    );

    // Compute the *next* anchor before any side effect so a transient
    // failure doesn't leave us stuck on the same past timestamp.
    let next_anchor = cadence::round_forward(
        schedule.cadence,
        schedule.time_of_day_minutes,
        now,
        now,
    );

    // Gate 1: bundled scanner must be available + integrity-valid.
    let scanner_ok = matches!(
        scanner::detect_binary(),
        scanner::ScoutSuiteAvailability::Available { .. },
    );
    if !scanner_ok {
        let _ = storage::record_run(
            account_id,
            now,
            LastRunOutcome::SkippedScannerUnavailable,
            None,
            next_anchor,
        );
        let _ = storage::append_event(
            &mint_event_id(),
            account_id,
            now,
            ScheduleEventKind::Skipped,
            Some("scanner_unavailable"),
            None,
        );
        return;
    }

    // Gate 2: the account must still exist (it could have been removed
    // between bootstrap and now even though the cascade in `accounts`
    // also removes the schedule — defense in depth).
    if accounts::get_account(account_id).is_err() {
        // No schedule row to advance — it was cascaded with the account.
        return;
    }

    // Gate 3: the account must have a provisioned scanner role.
    let account = match accounts::get_account(account_id) {
        Ok(a) => a,
        Err(_) => return,
    };
    if !account.role_provisioned {
        let _ = storage::record_run(
            account_id,
            now,
            LastRunOutcome::SkippedRoleNotProvisioned,
            None,
            next_anchor,
        );
        let _ = storage::append_event(
            &mint_event_id(),
            account_id,
            now,
            ScheduleEventKind::Skipped,
            Some("role_not_provisioned"),
            None,
        );
        return;
    }

    // Build a short-lived tokio runtime for the async `run_scan` call.
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(_) => return,
    };
    let outcome = rt.block_on(scanner::run_scan(account_id));
    match outcome {
        Ok(record) => {
            let final_outcome = if is_catch_up {
                LastRunOutcome::CatchUp
            } else {
                LastRunOutcome::Fired
            };
            let _ = storage::record_run(
                account_id,
                now,
                final_outcome,
                Some(&record.scan_id),
                next_anchor,
            );
            let kind = if is_catch_up {
                ScheduleEventKind::CatchUp
            } else {
                ScheduleEventKind::Fired
            };
            let _ = storage::append_event(
                &mint_event_id(),
                account_id,
                now,
                kind,
                None,
                Some(&record.scan_id),
            );
        }
        Err(scanner::ScannerError::AlreadyRunning) => {
            let _ = storage::record_run(
                account_id,
                now,
                LastRunOutcome::SkippedAlreadyRunning,
                None,
                next_anchor,
            );
            let _ = storage::append_event(
                &mint_event_id(),
                account_id,
                now,
                ScheduleEventKind::Skipped,
                Some("already_running"),
                None,
            );
        }
        Err(scanner::ScannerError::RoleNotProvisioned) => {
            let _ = storage::record_run(
                account_id,
                now,
                LastRunOutcome::SkippedRoleNotProvisioned,
                None,
                next_anchor,
            );
            let _ = storage::append_event(
                &mint_event_id(),
                account_id,
                now,
                ScheduleEventKind::Skipped,
                Some("role_not_provisioned"),
                None,
            );
        }
        Err(scanner::ScannerError::NotBundled)
        | Err(scanner::ScannerError::IntegrityFailed) => {
            let _ = storage::record_run(
                account_id,
                now,
                LastRunOutcome::SkippedScannerUnavailable,
                None,
                next_anchor,
            );
            let _ = storage::append_event(
                &mint_event_id(),
                account_id,
                now,
                ScheduleEventKind::Skipped,
                Some("scanner_unavailable"),
                None,
            );
        }
        Err(_) => {
            let _ = storage::record_run(
                account_id,
                now,
                LastRunOutcome::SkippedInternalError,
                None,
                next_anchor,
            );
            let _ = storage::append_event(
                &mint_event_id(),
                account_id,
                now,
                ScheduleEventKind::Skipped,
                Some("internal_error"),
                None,
            );
        }
    }
}

fn last_event_kind(aws_account_id: &str) -> Option<ScheduleEventKind> {
    storage::recent_events(aws_account_id, 1)
        .ok()?
        .into_iter()
        .next()
        .map(|e| e.kind)
}

pub fn mint_event_id() -> String {
    let mut bytes = [0u8; 16];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Test-only: reset the "runner has started" guard so an integration test
/// can reinitialize state without leaking across tests. Production code
/// never calls this.
pub fn _reset_for_tests() {
    STARTED.store(false, Ordering::SeqCst);
}
