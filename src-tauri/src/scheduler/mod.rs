// Scheduler — Contract 10.
//
// Tracks per-account scan cadences in SQLite, runs a background poll loop
// that dispatches due scans through the existing scanner orchestrator
// (Contract 06), and records every fire/skip/config change so the event log
// (Contract 11) can surface scheduled-scan activity.
//
// Public surface (mirrors Contract 10 §Expected Output):
//
//     set_schedule(input)                  -> Schedule
//     get_schedule(aws_account_id)         -> Schedule
//     clear_schedule(aws_account_id)       -> ()
//     list_schedules()                     -> Vec<Schedule>
//     next_run_times()                     -> Vec<NextRunTime>
//     recent_events(aws_account_id, limit) -> Vec<ScheduleEvent>
//
// Things this module DOES NOT do (and never will):
//   - Add a second scan path. Scheduled scans go through the SAME
//     `scanner::run_scan` path as manual scans (CLAUDE.md §4.3:
//     fresh AssumeRole, verified binary, no credential persistence).
//   - Bypass the app lock's security model. The background runner runs in
//     the same process and respects the same gates. The lock controls UI
//     access, not background timers — Contract 10 §Constraints make this
//     decision explicit.
//   - Stack missed runs. At most one catch-up scan fires after the app
//     resumes across one or more missed scheduled times.

pub mod cadence;
pub mod error;
pub mod runner;
pub mod storage;
pub mod types;

pub use error::SchedulerError;
pub use types::{
    LastRunOutcome, NextRunTime, Schedule, ScheduleCadence, ScheduleEvent, ScheduleEventKind,
    SetScheduleInput,
};

use chrono::Utc;

use crate::accounts;

/// Configure a schedule for an account. Replaces any prior schedule for the
/// same account. Inputs are validated; the account must already exist in
/// the `accounts` table.
pub fn set_schedule(input: SetScheduleInput) -> Result<Schedule, SchedulerError> {
    validate_account_id(&input.aws_account_id)?;
    cadence::validate(input.cadence, input.time_of_day_minutes)
        .map_err(SchedulerError::InvalidInput)?;

    // Confirm the account exists. We don't ask `accounts::get_account` for
    // anything beyond presence — Contract 10 §Constraints only require that
    // schedules survive across the secure scan path, and that path itself
    // re-resolves the account at fire time.
    match accounts::get_account(&input.aws_account_id) {
        Ok(_) => {}
        Err(accounts::AccountsError::NotFound) => {
            return Err(SchedulerError::AccountNotFound)
        }
        Err(e) => return Err(SchedulerError::Accounts(e)),
    }

    // Was there a prior schedule? We need this to:
    //   * record an `enabled`/`disabled` event when the flag flips
    //   * preserve nothing else — full replace semantics for the row
    let prior = storage::get(&input.aws_account_id)?;

    // Precompute next_run_at — only if enabled. A disabled row gets a
    // None anchor so the runner ignores it (and the storage index still
    // works because `next_run_at` is nullable).
    let next = if input.enabled {
        cadence::next_after(input.cadence, input.time_of_day_minutes, Utc::now())
    } else {
        None
    };

    let row = storage::upsert(
        &input.aws_account_id,
        input.cadence,
        input.time_of_day_minutes,
        input.enabled,
        next,
    )?;

    // Lifecycle events for the event log (Contract 11 read path):
    //   * config_set fires on every set_schedule call (new or replace)
    //   * enabled/disabled fires when the flag flips relative to the
    //     prior row's `enabled`
    let now = Utc::now();
    let _ = storage::append_event(
        &runner::mint_event_id(),
        &input.aws_account_id,
        now,
        ScheduleEventKind::ConfigSet,
        Some(input.cadence.kind_str()),
        None,
    );
    if let Some(prior) = prior {
        if prior.enabled && !input.enabled {
            let _ = storage::append_event(
                &runner::mint_event_id(),
                &input.aws_account_id,
                now,
                ScheduleEventKind::Disabled,
                None,
                None,
            );
        } else if !prior.enabled && input.enabled {
            let _ = storage::append_event(
                &runner::mint_event_id(),
                &input.aws_account_id,
                now,
                ScheduleEventKind::Enabled,
                None,
                None,
            );
        }
    } else if input.enabled {
        let _ = storage::append_event(
            &runner::mint_event_id(),
            &input.aws_account_id,
            now,
            ScheduleEventKind::Enabled,
            None,
            None,
        );
    }

    Ok(row)
}

/// Read the configured schedule for an account. Returns
/// `SchedulerError::NotFound` when the account has none configured.
pub fn get_schedule(aws_account_id: &str) -> Result<Schedule, SchedulerError> {
    validate_account_id(aws_account_id)?;
    storage::get(aws_account_id)?.ok_or(SchedulerError::NotFound)
}

/// Remove a schedule. Idempotent — `NotFound` if no row exists. Used by
/// the Settings UI's "Clear schedule" button and by the accounts module
/// when an account is removed.
pub fn clear_schedule(aws_account_id: &str) -> Result<(), SchedulerError> {
    validate_account_id(aws_account_id)?;
    let deleted = storage::delete(aws_account_id)?;
    if !deleted {
        return Err(SchedulerError::NotFound);
    }
    let _ = storage::append_event(
        &runner::mint_event_id(),
        aws_account_id,
        Utc::now(),
        ScheduleEventKind::ConfigCleared,
        None,
        None,
    );
    Ok(())
}

/// Remove a schedule WITHOUT erroring if it doesn't exist. Used by the
/// accounts removal path so a no-schedule account doesn't surface a fake
/// NotFound from removal.
pub fn clear_schedule_if_present(aws_account_id: &str) -> Result<bool, SchedulerError> {
    validate_account_id(aws_account_id)?;
    let deleted = storage::delete(aws_account_id)?;
    if deleted {
        let _ = storage::append_event(
            &runner::mint_event_id(),
            aws_account_id,
            Utc::now(),
            ScheduleEventKind::ConfigCleared,
            None,
            None,
        );
    }
    Ok(deleted)
}

/// All schedules, ordered by account ID. Drives the Settings UI list.
pub fn list_schedules() -> Result<Vec<Schedule>, SchedulerError> {
    storage::list()
}

/// Upcoming-run timestamps for every schedule. Includes disabled schedules
/// (with `next_run_at = None`) so the UI can render the full table.
pub fn next_run_times() -> Result<Vec<NextRunTime>, SchedulerError> {
    let all = storage::list()?;
    let out = all
        .into_iter()
        .map(|s| NextRunTime {
            aws_account_id: s.aws_account_id,
            next_run_at: if s.enabled { s.next_run_at } else { None },
        })
        .collect();
    Ok(out)
}

/// Read the N most recent schedule_events for an account. Used by the UI
/// to show "skipped at … (already running)" history.
pub fn recent_events(
    aws_account_id: &str,
    limit: usize,
) -> Result<Vec<ScheduleEvent>, SchedulerError> {
    validate_account_id(aws_account_id)?;
    storage::recent_events(aws_account_id, limit)
}

/// Validate an AWS account ID at the scheduler boundary. Re-applies the same
/// 12-digit grammar the scanner module enforces so a malformed string can't
/// become a SQL primary key.
fn validate_account_id(id: &str) -> Result<(), SchedulerError> {
    if id.len() == 12 && id.chars().all(|c| c.is_ascii_digit()) {
        Ok(())
    } else {
        Err(SchedulerError::InvalidInput("aws_account_id"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_account_id_rejects_non_digit_input() {
        assert!(validate_account_id("12345678901").is_err());
        assert!(validate_account_id("1234567890123").is_err());
        assert!(validate_account_id("abcd56789012").is_err());
        assert!(validate_account_id("111122223333").is_ok());
    }
}
