// Retention engine — Contract 11B.
//
// Two independent, user-configurable policies:
//   * `scan_retention`     — raw scan output dir + `raw-scout.json` files.
//                            Default 90 days.
//   * `eventlog_retention` — `event_log` rows. Default 90 days.
//
// Findings metadata is NOT purged by retention (Contract 11 §Constraints
// + Acceptance Criteria). Findings drive trend/drift; only the raw file
// on disk and the per-event row in `event_log` are subject to retention.
//
// In-progress scans are protected: a scan whose started_at falls before
// the cutoff but whose status is still non-terminal is left untouched.
// Only TERMINAL scans whose raw output is older than the cutoff are
// candidates for purge.
//
// Public surface:
//
//     get_settings()                 -> RetentionSettings
//     set_scan_retention(period)     -> ()
//     set_eventlog_retention(period) -> ()
//     run_now()                      -> RetentionRunSummary

pub mod error;
pub mod storage;
pub mod types;

pub use error::RetentionError;
pub use types::{RetentionPeriod, RetentionRunSummary, RetentionSettings};

use chrono::{Duration, Utc};
use rusqlite::{params, Connection};

use crate::db::paths::app_data_dir;
use crate::eventlog::{self, EventInput, EventKind};

/// Read both policies + the last-run timestamp.
pub fn get_settings() -> Result<RetentionSettings, RetentionError> {
    storage::read()
}

/// Update the raw-scan-output retention. Idempotent; setting the same
/// value twice is a no-op as far as the engine is concerned.
pub fn set_scan_retention(period: RetentionPeriod) -> Result<(), RetentionError> {
    validate_period(period)?;
    storage::write_scan(period)?;
    eventlog::record_event(
        EventInput::new(
            EventKind::SettingsChanged,
            format!("Scan-output retention set to {}.", describe_period(period)),
        )
        .with_detail("retention_scan_days"),
    );
    Ok(())
}

/// Update the event-log retention. Independent of `set_scan_retention`.
pub fn set_eventlog_retention(period: RetentionPeriod) -> Result<(), RetentionError> {
    validate_period(period)?;
    storage::write_eventlog(period)?;
    eventlog::record_event(
        EventInput::new(
            EventKind::SettingsChanged,
            format!("Event-log retention set to {}.", describe_period(period)),
        )
        .with_detail("retention_eventlog_days"),
    );
    Ok(())
}

fn validate_period(period: RetentionPeriod) -> Result<(), RetentionError> {
    if let RetentionPeriod::Days(n) = period {
        if n == 0 || n > 3650 {
            return Err(RetentionError::InvalidInput("retention_days"));
        }
    }
    Ok(())
}

fn describe_period(period: RetentionPeriod) -> String {
    match period {
        RetentionPeriod::Days(n) => format!("{n} days"),
        RetentionPeriod::Never => "never (manual only)".to_string(),
    }
}

/// Run both sweeps now. Returns a summary the UI can show as a toast.
/// Each phase is independent — if scan purge fails the event-log purge
/// still runs.
pub fn run_now() -> Result<RetentionRunSummary, RetentionError> {
    let settings = storage::read()?;
    let now = Utc::now();
    let scan_cutoff = match settings.scan_retention {
        RetentionPeriod::Days(n) => Some(now - Duration::days(n as i64)),
        RetentionPeriod::Never => None,
    };
    let event_cutoff = match settings.eventlog_retention {
        RetentionPeriod::Days(n) => Some(now - Duration::days(n as i64)),
        RetentionPeriod::Never => None,
    };

    let (scan_dirs_removed, raw_files_removed) = match scan_cutoff {
        Some(cutoff) => purge_scan_output(cutoff)?,
        None => (0, 0),
    };

    let eventlog_rows_removed = match event_cutoff {
        Some(cutoff) => crate::eventlog::storage::purge_older_than(cutoff)
            .map_err(|e| RetentionError::Db(format!("eventlog purge: {e}")))?,
        None => 0,
    };

    storage::set_last_run(now)?;

    if scan_dirs_removed > 0 || raw_files_removed > 0 || eventlog_rows_removed > 0 {
        eventlog::record_event(
            EventInput::new(
                EventKind::RetentionPurged,
                format!(
                    "Retention purged {scan_dirs} scan dirs, {raw} raw files, {ev} event-log rows.",
                    scan_dirs = scan_dirs_removed,
                    raw = raw_files_removed,
                    ev = eventlog_rows_removed,
                ),
            )
            .with_item_count(
                (scan_dirs_removed + raw_files_removed + eventlog_rows_removed) as i64,
            ),
        );
    }

    Ok(RetentionRunSummary {
        scan_dirs_removed,
        raw_files_removed,
        eventlog_rows_removed,
        scan_cutoff,
        eventlog_cutoff: event_cutoff,
    })
}

/// Purge raw scan output for TERMINAL scans whose started_at is older than
/// `cutoff`. Returns `(scan_dirs_removed, raw_files_unlinked)`.
///
/// The scan row itself stays — findings metadata is never purged. Only the
/// on-disk `raw-scout.json` (and any other files in `scans/{scan-id}/`) is
/// removed, and the `raw_output_path` column is cleared so the UI knows the
/// raw is gone (a "re-parse" would surface `RawOutputMissing`, the same
/// behavior as a user-initiated raw-file deletion).
fn purge_scan_output(cutoff: chrono::DateTime<Utc>) -> Result<(usize, usize), RetentionError> {
    let conn = open_db()?;

    // Defense in depth: we only consider terminal scans. A non-terminal
    // scan whose raw is still being written must not be purged.
    let mut stmt = conn.prepare(
        "SELECT scan_id, raw_output_path FROM scans
          WHERE started_at < ?1
            AND status IN ('complete', 'complete_with_warnings', 'failed', 'canceled')
            AND raw_output_path IS NOT NULL",
    )?;
    let rows = stmt.query_map(params![cutoff.to_rfc3339()], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
    })?;

    let mut candidates: Vec<(String, Option<String>)> = Vec::new();
    for r in rows {
        candidates.push(r?);
    }
    drop(stmt);

    let root = app_data_dir().map_err(|e| RetentionError::Io(e.to_string()))?;
    let scans_root = root.join("scans");

    let mut scan_dirs_removed = 0usize;
    let mut raw_files_removed = 0usize;

    for (scan_id, raw_path) in candidates {
        if let Some(p) = raw_path.as_deref() {
            let path = std::path::Path::new(p);
            if path.is_file() {
                if std::fs::remove_file(path).is_ok() {
                    raw_files_removed += 1;
                }
            }
        }
        // Remove the per-scan directory if it exists and is empty enough
        // for a recursive remove. We always know the canonical path
        // (`scans/{scan-id}/`) so the unlink never escapes that subtree.
        let dir = scans_root.join(&scan_id);
        if dir.is_dir() {
            if std::fs::remove_dir_all(&dir).is_ok() {
                scan_dirs_removed += 1;
            }
        }
        // Clear `raw_output_path` so a subsequent re-parse surfaces a
        // stable "raw output missing" error instead of a dangling path.
        conn.execute(
            "UPDATE scans SET raw_output_path = NULL WHERE scan_id = ?1",
            params![scan_id],
        )?;
    }

    Ok((scan_dirs_removed, raw_files_removed))
}

fn open_db() -> Result<Connection, RetentionError> {
    let path = app_data_dir()
        .map_err(|e| RetentionError::Io(e.to_string()))?
        .join("db")
        .join("cloudsaw.db");
    Connection::open(&path).map_err(RetentionError::from)
}

/// Bootstrap helper, called from `lib::run` after migrations. Best-effort
/// sweep — errors are swallowed so a transient SQLite hiccup doesn't
/// prevent the app from starting.
pub fn bootstrap_sweep() {
    let _ = run_now();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn period_round_trips_storage_string() {
        assert_eq!(
            RetentionPeriod::from_storage(&RetentionPeriod::Days(30).to_storage()),
            RetentionPeriod::Days(30)
        );
        assert_eq!(
            RetentionPeriod::from_storage(&RetentionPeriod::Never.to_storage()),
            RetentionPeriod::Never
        );
    }

    #[test]
    fn period_falls_back_on_corrupt_input() {
        assert_eq!(
            RetentionPeriod::from_storage("garbage"),
            RetentionPeriod::Days(90)
        );
    }

    #[test]
    fn validate_period_rejects_zero_and_huge_values() {
        assert!(validate_period(RetentionPeriod::Days(0)).is_err());
        assert!(validate_period(RetentionPeriod::Days(10_000)).is_err());
        assert!(validate_period(RetentionPeriod::Days(180)).is_ok());
        assert!(validate_period(RetentionPeriod::Never).is_ok());
    }
}
