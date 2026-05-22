// Settings-table reads/writes for retention configuration.
//
// We piggyback on the `settings` key/value table (migration 0001) rather
// than adding a dedicated retention table — the configuration is three
// small values and the existing table already exists for exactly this
// kind of singleton setting.

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};

use super::error::RetentionError;
use super::types::{RetentionPeriod, RetentionSettings};
use crate::db::paths::app_data_dir;

const KEY_SCAN: &str = "retention_scan_days";
const KEY_EVENT: &str = "retention_eventlog_days";
const KEY_LAST_RUN: &str = "retention_last_run_at";

fn db_path() -> Result<std::path::PathBuf, RetentionError> {
    Ok(app_data_dir()
        .map_err(|e| RetentionError::Io(e.to_string()))?
        .join("db")
        .join("cloudsaw.db"))
}

fn open() -> Result<Connection, RetentionError> {
    Connection::open(db_path()?).map_err(RetentionError::from)
}

pub fn read() -> Result<RetentionSettings, RetentionError> {
    let conn = open()?;
    let scan = get_value_with(&conn, KEY_SCAN)?
        .as_deref()
        .map(RetentionPeriod::from_storage)
        .unwrap_or(RetentionPeriod::Days(90));
    let event = get_value_with(&conn, KEY_EVENT)?
        .as_deref()
        .map(RetentionPeriod::from_storage)
        .unwrap_or(RetentionPeriod::Days(90));
    let last_run_raw = get_value_with(&conn, KEY_LAST_RUN)?.unwrap_or_default();
    let last_run_at = if last_run_raw.trim().is_empty() {
        None
    } else {
        Some(
            DateTime::parse_from_rfc3339(&last_run_raw)
                .map_err(|e| RetentionError::Db(format!("bad last_run_at: {e}")))?
                .with_timezone(&Utc),
        )
    };

    Ok(RetentionSettings {
        scan_retention: scan,
        eventlog_retention: event,
        last_run_at,
    })
}

pub fn write_scan(period: RetentionPeriod) -> Result<(), RetentionError> {
    set_value(KEY_SCAN, &period.to_storage())
}

pub fn write_eventlog(period: RetentionPeriod) -> Result<(), RetentionError> {
    set_value(KEY_EVENT, &period.to_storage())
}

pub fn set_last_run(at: DateTime<Utc>) -> Result<(), RetentionError> {
    set_value(KEY_LAST_RUN, &at.to_rfc3339())
}

fn set_value(key: &str, value: &str) -> Result<(), RetentionError> {
    let conn = open()?;
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO settings (key, value, updated_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value,
                                        updated_at = excluded.updated_at",
        params![key, value, now],
    )?;
    Ok(())
}

fn get_value_with(conn: &Connection, key: &str) -> Result<Option<String>, RetentionError> {
    let row: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |r| r.get(0),
        )
        .optional()?;
    Ok(row)
}
