// SQLite-backed read/write for the `scans` table (Contract 06).
//
// Every function opens its own connection. Scans land on user cadence
// (start, poll-every-second, cancel) — the simpler per-call connection is
// fine and avoids a shared `Mutex<Connection>` other modules would have to
// fight over. The same pattern as `accounts::storage`.
//
// CLAUDE.md §4.5: every SQL here uses parameterized queries.

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};

use super::error::ScannerError;
use super::types::{ScanRecord, ScanStatus};
use crate::accounts::ScanOutcome;
use crate::db::paths::app_data_dir;

fn db_path() -> Result<std::path::PathBuf, ScannerError> {
    Ok(app_data_dir()
        .map_err(|e| ScannerError::ScanIo(e.to_string()))?
        .join("db")
        .join("cloudsaw.db"))
}

fn open() -> Result<Connection, ScannerError> {
    Connection::open(db_path()?).map_err(ScannerError::from)
}

/// Insert a brand-new scan row in `pending`. The orchestrator transitions it
/// out of pending almost immediately, but the row lands before the spawn so
/// `scan_status` can be polled even if the orchestrator panics mid-setup.
pub fn insert_pending(
    scan_id: &str,
    aws_account_id: &str,
    role_session_name: &str,
) -> Result<ScanRecord, ScannerError> {
    let conn = open()?;
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO scans (
            scan_id, aws_account_id, status, started_at,
            role_session_name, truncated
         ) VALUES (?1, ?2, ?3, ?4, ?5, 0)",
        params![
            scan_id,
            aws_account_id,
            ScanStatus::Pending.as_str(),
            now,
            role_session_name,
        ],
    )?;
    get(scan_id)
}

/// Update only the `status` column. Used for the in-flight transitions
/// (`pending → assuming_role → scanning → parsing`).
pub fn update_status(scan_id: &str, status: ScanStatus) -> Result<(), ScannerError> {
    let conn = open()?;
    let affected = conn.execute(
        "UPDATE scans SET status = ?1 WHERE scan_id = ?2",
        params![status.as_str(), scan_id],
    )?;
    if affected == 0 {
        return Err(ScannerError::ScanNotFound);
    }
    Ok(())
}

/// Record the path to `raw-scout.json` once the scanner has produced it.
/// Called from the `parsing` transition so the path is durable before any
/// terminal-status update.
pub fn set_raw_output_path(scan_id: &str, path: &str) -> Result<(), ScannerError> {
    let conn = open()?;
    let affected = conn.execute(
        "UPDATE scans SET raw_output_path = ?1 WHERE scan_id = ?2",
        params![path, scan_id],
    )?;
    if affected == 0 {
        return Err(ScannerError::ScanNotFound);
    }
    Ok(())
}

/// Persist the PID of the running ScoutSuite child. Surfaced via
/// `scan_status` so QA can confirm a child is alive and the cancellation
/// path has a target. Set to `None` once the child is reaped.
pub fn set_pid(scan_id: &str, pid: Option<u32>) -> Result<(), ScannerError> {
    let conn = open()?;
    let pid_val: Option<i64> = pid.map(|p| p as i64);
    let affected = conn.execute(
        "UPDATE scans SET pid = ?1 WHERE scan_id = ?2",
        params![pid_val, scan_id],
    )?;
    if affected == 0 {
        return Err(ScannerError::ScanNotFound);
    }
    Ok(())
}

/// Mark a scan as terminally succeeded. Optionally records a warning code
/// (e.g. "missing_permissions") for the `complete_with_warnings` branch.
/// Also updates the parent `accounts` row so the home/accounts views can
/// display the latest scan outcome.
pub fn record_complete(
    scan_id: &str,
    with_warnings: Option<(&str, Option<&str>)>,
    truncated: bool,
) -> Result<(), ScannerError> {
    let mut conn = open()?;
    let tx = conn.transaction()?;
    let now = Utc::now().to_rfc3339();

    let (status, warn_code, warn_detail, outcome) = match with_warnings {
        Some((code, detail)) => (
            ScanStatus::CompleteWithWarnings,
            Some(code.to_string()),
            detail.map(|s| s.to_string()),
            ScanOutcome::PartialSuccess,
        ),
        None => (ScanStatus::Complete, None, None, ScanOutcome::Success),
    };

    let truncated_val: i64 = if truncated { 1 } else { 0 };
    let affected = tx.execute(
        "UPDATE scans
            SET status = ?1,
                finished_at = ?2,
                warning_code = ?3,
                warning_detail = ?4,
                truncated = ?5,
                pid = NULL
          WHERE scan_id = ?6",
        params![
            status.as_str(),
            now,
            warn_code,
            warn_detail,
            truncated_val,
            scan_id,
        ],
    )?;
    if affected == 0 {
        return Err(ScannerError::ScanNotFound);
    }

    // Mirror the outcome onto the accounts row so the UI shows the latest
    // status without a join.
    let aws_account_id = scan_account_id_within(&tx, scan_id)?;
    tx.execute(
        "UPDATE accounts
            SET last_scan_at = ?1,
                last_scan_status = ?2,
                updated_at = ?1
          WHERE aws_account_id = ?3",
        params![now, outcome_as_str(outcome), aws_account_id],
    )?;

    tx.commit()?;
    Ok(())
}

/// Mark a scan as terminally failed with a stable error code.
pub fn record_failed(scan_id: &str, failure_code: &str) -> Result<(), ScannerError> {
    let mut conn = open()?;
    let tx = conn.transaction()?;
    let now = Utc::now().to_rfc3339();

    let affected = tx.execute(
        "UPDATE scans
            SET status = ?1,
                finished_at = ?2,
                failure_code = ?3,
                pid = NULL
          WHERE scan_id = ?4",
        params![
            ScanStatus::Failed.as_str(),
            now,
            failure_code,
            scan_id,
        ],
    )?;
    if affected == 0 {
        return Err(ScannerError::ScanNotFound);
    }

    let aws_account_id = scan_account_id_within(&tx, scan_id)?;
    tx.execute(
        "UPDATE accounts
            SET last_scan_at = ?1,
                last_scan_status = ?2,
                updated_at = ?1
          WHERE aws_account_id = ?3",
        params![now, outcome_as_str(ScanOutcome::Failure), aws_account_id],
    )?;

    tx.commit()?;
    Ok(())
}

/// Mark a scan as canceled. Partial output is preserved on disk; the
/// `truncated` flag is propagated unchanged because cancellation can happen
/// at any stage.
pub fn record_canceled(scan_id: &str) -> Result<(), ScannerError> {
    let mut conn = open()?;
    let tx = conn.transaction()?;
    let now = Utc::now().to_rfc3339();

    let affected = tx.execute(
        "UPDATE scans
            SET status = ?1,
                finished_at = ?2,
                pid = NULL
          WHERE scan_id = ?3",
        params![ScanStatus::Canceled.as_str(), now, scan_id],
    )?;
    if affected == 0 {
        return Err(ScannerError::ScanNotFound);
    }

    let aws_account_id = scan_account_id_within(&tx, scan_id)?;
    tx.execute(
        "UPDATE accounts
            SET last_scan_at = ?1,
                last_scan_status = ?2,
                updated_at = ?1
          WHERE aws_account_id = ?3",
        params![now, outcome_as_str(ScanOutcome::Unknown), aws_account_id],
    )?;

    tx.commit()?;
    Ok(())
}

/// Read a scan row by ID.
pub fn get(scan_id: &str) -> Result<ScanRecord, ScannerError> {
    let conn = open()?;
    conn.query_row(
        "SELECT scan_id, aws_account_id, status, started_at, finished_at,
                failure_code, warning_code, warning_detail, raw_output_path,
                role_session_name, truncated
           FROM scans
          WHERE scan_id = ?1",
        params![scan_id],
        row_to_record,
    )
    .optional()?
    .ok_or(ScannerError::ScanNotFound)
}

/// Return the most recent N scans for an account. Used by the UI history
/// surface. Ordered by `started_at DESC`.
pub fn list_for_account(
    aws_account_id: &str,
    limit: usize,
) -> Result<Vec<ScanRecord>, ScannerError> {
    let conn = open()?;
    let mut stmt = conn.prepare(
        "SELECT scan_id, aws_account_id, status, started_at, finished_at,
                failure_code, warning_code, warning_detail, raw_output_path,
                role_session_name, truncated
           FROM scans
          WHERE aws_account_id = ?1
          ORDER BY started_at DESC
          LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![aws_account_id, limit as i64], row_to_record)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// True if the account has a scan whose status is not terminal. Drives the
/// "scan already running" gate. The orchestrator MUST consult this before
/// inserting a new pending row, and the storage layer enforces it again via
/// a transactional check (`try_claim_account`).
pub fn account_has_in_flight(aws_account_id: &str) -> Result<bool, ScannerError> {
    let conn = open()?;
    let row: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM scans
              WHERE aws_account_id = ?1
                AND status NOT IN ('complete', 'complete_with_warnings', 'failed', 'canceled')
              LIMIT 1",
            params![aws_account_id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(row.is_some())
}

/// Single-shot insert that also rejects if any in-flight scan exists. The
/// concurrent-scan rule lands here transactionally so a second `run_scan`
/// call cannot win a race against the first.
pub fn try_claim_account(
    scan_id: &str,
    aws_account_id: &str,
    role_session_name: &str,
) -> Result<ScanRecord, ScannerError> {
    let mut conn = open()?;
    let tx = conn.transaction()?;

    let busy: Option<i64> = tx
        .query_row(
            "SELECT 1 FROM scans
              WHERE aws_account_id = ?1
                AND status NOT IN ('complete', 'complete_with_warnings', 'failed', 'canceled')
              LIMIT 1",
            params![aws_account_id],
            |r| r.get(0),
        )
        .optional()?;
    if busy.is_some() {
        return Err(ScannerError::AlreadyRunning);
    }

    let now = Utc::now().to_rfc3339();
    tx.execute(
        "INSERT INTO scans (
            scan_id, aws_account_id, status, started_at,
            role_session_name, truncated
         ) VALUES (?1, ?2, ?3, ?4, ?5, 0)",
        params![
            scan_id,
            aws_account_id,
            ScanStatus::Pending.as_str(),
            now,
            role_session_name,
        ],
    )?;
    tx.commit()?;
    get(scan_id)
}

/// Recover-on-launch helper: find every row whose status is non-terminal
/// and mark it `failed` with `scanner_process_lost`. Contract 06 §Edge
/// Cases: "the machine sleeps mid-scan and the child process is lost → on
/// resume the scan is detected as stale and marked `failed` with a
/// process-lost reason."
pub fn reap_stale_in_flight() -> Result<usize, ScannerError> {
    let mut conn = open()?;
    let tx = conn.transaction()?;
    let now = Utc::now().to_rfc3339();

    let mut stmt = tx.prepare(
        "SELECT scan_id, aws_account_id FROM scans
          WHERE status NOT IN ('complete', 'complete_with_warnings', 'failed', 'canceled')",
    )?;
    let rows: Vec<(String, String)> = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<_, _>>()?;
    drop(stmt);

    for (scan_id, aws_account_id) in &rows {
        tx.execute(
            "UPDATE scans
                SET status = ?1,
                    finished_at = ?2,
                    failure_code = ?3,
                    pid = NULL
              WHERE scan_id = ?4",
            params![
                ScanStatus::Failed.as_str(),
                now,
                "scanner_process_lost",
                scan_id,
            ],
        )?;
        tx.execute(
            "UPDATE accounts
                SET last_scan_at = ?1,
                    last_scan_status = ?2,
                    updated_at = ?1
              WHERE aws_account_id = ?3",
            params![now, outcome_as_str(ScanOutcome::Failure), aws_account_id],
        )?;
    }
    tx.commit()?;
    Ok(rows.len())
}

fn scan_account_id_within(
    tx: &rusqlite::Transaction<'_>,
    scan_id: &str,
) -> Result<String, ScannerError> {
    tx.query_row(
        "SELECT aws_account_id FROM scans WHERE scan_id = ?1",
        params![scan_id],
        |r| r.get::<_, String>(0),
    )
    .optional()?
    .ok_or(ScannerError::ScanNotFound)
}

fn outcome_as_str(o: ScanOutcome) -> &'static str {
    match o {
        ScanOutcome::Success => "success",
        ScanOutcome::Failure => "failure",
        ScanOutcome::PartialSuccess => "partial_success",
        ScanOutcome::Unknown => "unknown",
    }
}

fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<ScanRecord> {
    let scan_id: String = row.get(0)?;
    let aws_account_id: String = row.get(1)?;
    let status_str: String = row.get(2)?;
    let started_at: String = row.get(3)?;
    let finished_at: Option<String> = row.get(4)?;
    let failure_code: Option<String> = row.get(5)?;
    let warning_code: Option<String> = row.get(6)?;
    let warning_detail: Option<String> = row.get(7)?;
    let raw_output_path: Option<String> = row.get(8)?;
    let role_session_name: String = row.get(9)?;
    let truncated: i64 = row.get(10)?;

    let status = ScanStatus::from_storage(&status_str).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "unknown scan status",
            )),
        )
    })?;

    Ok(ScanRecord {
        scan_id,
        aws_account_id,
        status,
        started_at: parse_required_ts(started_at)?,
        finished_at: parse_optional_ts(finished_at)?,
        failure_code,
        warning_code,
        warning_detail,
        raw_output_path,
        role_session_name,
        truncated: truncated != 0,
    })
}

fn parse_required_ts(s: String) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(e),
            )
        })
}

fn parse_optional_ts(s: Option<String>) -> rusqlite::Result<Option<DateTime<Utc>>> {
    match s {
        None => Ok(None),
        Some(v) => parse_required_ts(v).map(Some),
    }
}
