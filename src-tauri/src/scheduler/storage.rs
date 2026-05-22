// SQLite-backed read/write for the `schedules` and `schedule_events` tables.
//
// The scheduler hits SQLite on user-action cadence (set/clear/list) plus a
// background-runner poll (default every 30 seconds). Per-call connections
// match the pattern used by `accounts::storage` and `scanner::storage` —
// no shared `Mutex<Connection>` other modules would have to fight over.
//
// CLAUDE.md §4.5: every SQL statement uses parameterized queries.

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};

use super::error::SchedulerError;
use super::types::{LastRunOutcome, Schedule, ScheduleCadence, ScheduleEvent, ScheduleEventKind};
use crate::db::paths::app_data_dir;

fn db_path() -> Result<std::path::PathBuf, SchedulerError> {
    Ok(app_data_dir()
        .map_err(|e| SchedulerError::Db(e.to_string()))?
        .join("db")
        .join("cloudsaw.db"))
}

fn open() -> Result<Connection, SchedulerError> {
    Connection::open(db_path()?).map_err(SchedulerError::from)
}

/// Insert-or-replace a schedule row for `aws_account_id`. Used by
/// `set_schedule`; the caller has already validated inputs and confirmed
/// the account exists. Computes/sets `created_at` on first insert,
/// preserving the original `created_at` on subsequent updates.
pub fn upsert(
    aws_account_id: &str,
    cadence: ScheduleCadence,
    time_of_day_minutes: Option<u16>,
    enabled: bool,
    next_run_at: Option<DateTime<Utc>>,
) -> Result<Schedule, SchedulerError> {
    let mut conn = open()?;
    let tx = conn.transaction()?;
    let now = Utc::now().to_rfc3339();

    let existing_created: Option<String> = tx
        .query_row(
            "SELECT created_at FROM schedules WHERE aws_account_id = ?1",
            params![aws_account_id],
            |r| r.get(0),
        )
        .optional()?;

    let created_at = existing_created.unwrap_or_else(|| now.clone());
    let next_run_str = next_run_at.map(|d| d.to_rfc3339());

    tx.execute(
        "INSERT INTO schedules (
            aws_account_id, cadence_kind, cadence_value, time_of_day_minutes,
            enabled, next_run_at, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(aws_account_id) DO UPDATE SET
            cadence_kind = excluded.cadence_kind,
            cadence_value = excluded.cadence_value,
            time_of_day_minutes = excluded.time_of_day_minutes,
            enabled = excluded.enabled,
            next_run_at = excluded.next_run_at,
            updated_at = excluded.updated_at",
        params![
            aws_account_id,
            cadence.kind_str(),
            cadence.cadence_value(),
            time_of_day_minutes.map(|t| t as i64),
            if enabled { 1 } else { 0 },
            next_run_str,
            created_at,
            now,
        ],
    )?;
    tx.commit()?;

    get(aws_account_id)?.ok_or(SchedulerError::Internal("upsert_lost_row"))
}

/// Read a schedule row by account ID. Returns `Ok(None)` when no schedule
/// is configured — distinct from `SchedulerError::NotFound`, which the public
/// API surfaces from `get_schedule`/`clear_schedule`.
pub fn get(aws_account_id: &str) -> Result<Option<Schedule>, SchedulerError> {
    let conn = open()?;
    let row = conn
        .query_row(
            "SELECT aws_account_id, cadence_kind, cadence_value,
                    time_of_day_minutes, enabled, last_run_at, last_run_outcome,
                    last_run_scan_id, next_run_at, created_at, updated_at
               FROM schedules
              WHERE aws_account_id = ?1",
            params![aws_account_id],
            row_to_schedule,
        )
        .optional()?;
    match row {
        None => Ok(None),
        Some(r) => Ok(Some(r?)),
    }
}

/// List every schedule, ordered by account ID for stable UI rendering.
pub fn list() -> Result<Vec<Schedule>, SchedulerError> {
    let conn = open()?;
    let mut stmt = conn.prepare(
        "SELECT aws_account_id, cadence_kind, cadence_value,
                time_of_day_minutes, enabled, last_run_at, last_run_outcome,
                last_run_scan_id, next_run_at, created_at, updated_at
           FROM schedules
       ORDER BY aws_account_id ASC",
    )?;
    let rows = stmt.query_map([], row_to_schedule)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r??);
    }
    Ok(out)
}

/// Delete a schedule. Returns `Ok(false)` when the row didn't exist; the
/// public `clear_schedule` translates that into `SchedulerError::NotFound`.
pub fn delete(aws_account_id: &str) -> Result<bool, SchedulerError> {
    let conn = open()?;
    let affected = conn.execute(
        "DELETE FROM schedules WHERE aws_account_id = ?1",
        params![aws_account_id],
    )?;
    Ok(affected > 0)
}

/// Update the precomputed next-run timestamp. Called by `set_schedule` and
/// by the background runner after each fire/skip.
pub fn set_next_run(
    aws_account_id: &str,
    next_run_at: Option<DateTime<Utc>>,
) -> Result<(), SchedulerError> {
    let conn = open()?;
    let now = Utc::now().to_rfc3339();
    let next_run_str = next_run_at.map(|d| d.to_rfc3339());
    let affected = conn.execute(
        "UPDATE schedules
            SET next_run_at = ?1, updated_at = ?2
          WHERE aws_account_id = ?3",
        params![next_run_str, now, aws_account_id],
    )?;
    if affected == 0 {
        return Err(SchedulerError::NotFound);
    }
    Ok(())
}

/// Record the most recent thing the runner did for an account: fired a scan
/// (`scan_id` populated), or skipped with a stable reason (`scan_id` None).
/// `next_run_at` is rewritten so subsequent polls don't reconsider the same
/// slot.
#[allow(clippy::too_many_arguments)]
pub fn record_run(
    aws_account_id: &str,
    occurred_at: DateTime<Utc>,
    outcome: LastRunOutcome,
    scan_id: Option<&str>,
    next_run_at: Option<DateTime<Utc>>,
) -> Result<(), SchedulerError> {
    let conn = open()?;
    let occurred_str = occurred_at.to_rfc3339();
    let next_str = next_run_at.map(|d| d.to_rfc3339());
    let affected = conn.execute(
        "UPDATE schedules
            SET last_run_at = ?1,
                last_run_outcome = ?2,
                last_run_scan_id = ?3,
                next_run_at = ?4,
                updated_at = ?1
          WHERE aws_account_id = ?5",
        params![
            occurred_str,
            outcome.as_str(),
            scan_id,
            next_str,
            aws_account_id,
        ],
    )?;
    if affected == 0 {
        return Err(SchedulerError::NotFound);
    }
    Ok(())
}

/// Append a schedule_events row. The runner and the public set/clear calls
/// both write here so the event log (Contract 11) surfaces every lifecycle
/// transition without reconstructing it.
pub fn append_event(
    event_id: &str,
    aws_account_id: &str,
    occurred_at: DateTime<Utc>,
    kind: ScheduleEventKind,
    reason: Option<&str>,
    scan_id: Option<&str>,
) -> Result<(), SchedulerError> {
    let conn = open()?;
    conn.execute(
        "INSERT INTO schedule_events (
            event_id, aws_account_id, occurred_at, kind, reason, scan_id
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            event_id,
            aws_account_id,
            occurred_at.to_rfc3339(),
            kind.as_str(),
            reason,
            scan_id,
        ],
    )?;
    Ok(())
}

/// Read the most recent N events for an account (newest first). Used by
/// the Settings UI to surface "fired at …", "skipped at … (reason)".
pub fn recent_events(
    aws_account_id: &str,
    limit: usize,
) -> Result<Vec<ScheduleEvent>, SchedulerError> {
    let conn = open()?;
    let bounded = limit.clamp(1, 200);
    let mut stmt = conn.prepare(
        "SELECT event_id, aws_account_id, occurred_at, kind, reason, scan_id
           FROM schedule_events
          WHERE aws_account_id = ?1
       ORDER BY occurred_at DESC
          LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![aws_account_id, bounded as i64], row_to_event)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r??);
    }
    Ok(out)
}

/// Every schedule that is currently enabled. Used by the background runner's
/// poll loop and by `next_run_times` to compute the upcoming timetable.
pub fn list_enabled() -> Result<Vec<Schedule>, SchedulerError> {
    let conn = open()?;
    let mut stmt = conn.prepare(
        "SELECT aws_account_id, cadence_kind, cadence_value,
                time_of_day_minutes, enabled, last_run_at, last_run_outcome,
                last_run_scan_id, next_run_at, created_at, updated_at
           FROM schedules
          WHERE enabled = 1
       ORDER BY aws_account_id ASC",
    )?;
    let rows = stmt.query_map([], row_to_schedule)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r??);
    }
    Ok(out)
}

fn row_to_schedule(row: &rusqlite::Row<'_>) -> rusqlite::Result<Result<Schedule, SchedulerError>> {
    let aws_account_id: String = row.get(0)?;
    let cadence_kind: String = row.get(1)?;
    let cadence_value: i64 = row.get(2)?;
    let time_of_day_minutes: Option<i64> = row.get(3)?;
    let enabled: i64 = row.get(4)?;
    let last_run_at: Option<String> = row.get(5)?;
    let last_run_outcome: Option<String> = row.get(6)?;
    let last_run_scan_id: Option<String> = row.get(7)?;
    let next_run_at: Option<String> = row.get(8)?;
    let created_at: String = row.get(9)?;
    let updated_at: String = row.get(10)?;

    let cadence = match ScheduleCadence::from_storage(&cadence_kind, cadence_value) {
        Some(c) => c,
        None => return Ok(Err(SchedulerError::Internal("bad_cadence_in_row"))),
    };
    let outcome = match last_run_outcome {
        None => None,
        Some(s) => match LastRunOutcome::from_storage(&s) {
            Some(o) => Some(o),
            None => return Ok(Err(SchedulerError::Internal("bad_last_outcome_in_row"))),
        },
    };
    let time_of_day = match time_of_day_minutes {
        None => None,
        Some(v) if (0..=1439).contains(&v) => Some(v as u16),
        _ => return Ok(Err(SchedulerError::Internal("bad_time_of_day_in_row"))),
    };

    Ok(Ok(Schedule {
        aws_account_id,
        cadence,
        time_of_day_minutes: time_of_day,
        enabled: enabled != 0,
        last_run_at: parse_optional_ts(last_run_at)?,
        last_run_outcome: outcome,
        last_run_scan_id,
        next_run_at: parse_optional_ts(next_run_at)?,
        created_at: parse_required_ts(created_at)?,
        updated_at: parse_required_ts(updated_at)?,
    }))
}

fn row_to_event(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<Result<ScheduleEvent, SchedulerError>> {
    let event_id: String = row.get(0)?;
    let aws_account_id: String = row.get(1)?;
    let occurred_at: String = row.get(2)?;
    let kind_str: String = row.get(3)?;
    let reason: Option<String> = row.get(4)?;
    let scan_id: Option<String> = row.get(5)?;

    let kind = match ScheduleEventKind::from_storage(&kind_str) {
        Some(k) => k,
        None => return Ok(Err(SchedulerError::Internal("bad_event_kind_in_row"))),
    };

    Ok(Ok(ScheduleEvent {
        event_id,
        aws_account_id,
        occurred_at: parse_required_ts(occurred_at)?,
        kind,
        reason,
        scan_id,
    }))
}

fn parse_required_ts(s: String) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })
}

fn parse_optional_ts(s: Option<String>) -> rusqlite::Result<Option<DateTime<Utc>>> {
    match s {
        None => Ok(None),
        Some(v) => parse_required_ts(v).map(Some),
    }
}
