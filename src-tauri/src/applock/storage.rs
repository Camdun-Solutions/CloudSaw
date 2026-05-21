// SQLite-backed read/write for the single `app_lock` row.
//
// Every function here opens its own connection and closes it on return.
// SQLite handles concurrency for us; app-lock operations are infrequent
// (first-run, unlock, settings change) so pooling isn't worth the complexity.
//
// CLAUDE.md §4.5: SQL is parameterized everywhere. No string interpolation.

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};

use crate::db::paths::app_data_dir;
use crate::errors::AppError;

/// Snapshot of the persisted lock state. All fields except `password_hash` are
/// safe to surface to the UI (the hash never crosses IPC).
#[derive(Debug, Clone)]
pub struct AppLockRow {
    pub password_hash: Option<String>,
    pub biometric_enabled: bool,
    /// `None` encodes "never re-lock"; `Some(0)` encodes "lock on close";
    /// `Some(n>0)` is the timed period in seconds.
    pub lock_period_seconds: Option<i64>,
    pub last_unlocked_at: Option<DateTime<Utc>>,
}

fn db_path() -> Result<std::path::PathBuf, AppError> {
    Ok(app_data_dir()?.join("db").join("cloudsaw.db"))
}

fn open() -> Result<Connection, AppError> {
    let path = db_path()?;
    Connection::open(&path).map_err(|e| AppError::Db(format!("open {}: {e}", path.display())))
}

pub fn read() -> Result<AppLockRow, AppError> {
    let conn = open()?;
    read_with(&conn)
}

pub fn read_with(conn: &Connection) -> Result<AppLockRow, AppError> {
    let row = conn
        .query_row(
            "SELECT password_hash, biometric_enabled, lock_period_seconds, last_unlocked_at
             FROM app_lock WHERE id = 1",
            [],
            |r| {
                let password_hash: Option<String> = r.get(0)?;
                let biometric_enabled: i64 = r.get(1)?;
                let lock_period_seconds: Option<i64> = r.get(2)?;
                let last_unlocked_at: Option<String> = r.get(3)?;
                Ok((
                    password_hash,
                    biometric_enabled != 0,
                    lock_period_seconds,
                    last_unlocked_at,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| AppError::Db("app_lock row missing — migration not applied?".into()))?;

    let last_unlocked_at = match row.3 {
        Some(s) => Some(
            DateTime::parse_from_rfc3339(&s)
                .map_err(|e| AppError::Db(format!("bad last_unlocked_at: {e}")))?
                .with_timezone(&Utc),
        ),
        None => None,
    };

    Ok(AppLockRow {
        password_hash: row.0,
        biometric_enabled: row.1,
        lock_period_seconds: row.2,
        last_unlocked_at,
    })
}

/// Write the password hash. Used by first-run setup, change-password, and
/// recovery flows. Pass `None` to clear (not currently exposed via IPC; kept
/// for tests).
pub fn set_password_hash(hash: Option<&str>) -> Result<(), AppError> {
    let conn = open()?;
    conn.execute(
        "UPDATE app_lock SET password_hash = ?1, updated_at = ?2 WHERE id = 1",
        params![hash, Utc::now().to_rfc3339()],
    )?;
    Ok(())
}

pub fn set_biometric_enabled(enabled: bool) -> Result<(), AppError> {
    let conn = open()?;
    conn.execute(
        "UPDATE app_lock SET biometric_enabled = ?1, updated_at = ?2 WHERE id = 1",
        params![enabled as i64, Utc::now().to_rfc3339()],
    )?;
    Ok(())
}

pub fn set_lock_period_seconds(period: Option<i64>) -> Result<(), AppError> {
    let conn = open()?;
    conn.execute(
        "UPDATE app_lock SET lock_period_seconds = ?1, updated_at = ?2 WHERE id = 1",
        params![period, Utc::now().to_rfc3339()],
    )?;
    Ok(())
}

pub fn record_unlock(now: DateTime<Utc>) -> Result<(), AppError> {
    let conn = open()?;
    conn.execute(
        "UPDATE app_lock SET last_unlocked_at = ?1, updated_at = ?2 WHERE id = 1",
        params![now.to_rfc3339(), now.to_rfc3339()],
    )?;
    Ok(())
}

/// Atomic change-password: verify the OLD hash, then write the NEW hash, in a
/// single transaction so a failure leaves the original password valid
/// (Contract 02 edge case).
pub fn replace_password_hash_atomic(
    expected_old_hash_check: impl FnOnce(&str) -> Result<bool, AppError>,
    new_hash: &str,
) -> Result<(), AppError> {
    let mut conn = open()?;
    let tx = conn.transaction()?;
    let current: Option<String> = tx
        .query_row("SELECT password_hash FROM app_lock WHERE id = 1", [], |r| {
            r.get(0)
        })
        .optional()?
        .ok_or_else(|| AppError::Db("app_lock row missing".into()))?;
    let current = current.ok_or(AppError::NotConfigured)?;

    if !expected_old_hash_check(&current)? {
        return Err(AppError::PasswordRejected);
    }

    tx.execute(
        "UPDATE app_lock SET password_hash = ?1, updated_at = ?2 WHERE id = 1",
        params![new_hash, Utc::now().to_rfc3339()],
    )?;
    tx.commit()?;
    Ok(())
}
