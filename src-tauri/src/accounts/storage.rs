// SQLite-backed read/write for the `accounts` and `active_account` tables.
//
// Every function opens its own connection. Multi-account ops happen on user
// action cadence (add/remove/switch), not in a hot loop, so the simpler
// per-call connection is fine and avoids a shared `Mutex<Connection>` that
// later contracts would have to fight over.
//
// CLAUDE.md §4.5: every SQL statement here uses parameterized queries. No
// string interpolation, anywhere.

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};

use super::error::AccountsError;
use super::types::{Account, AccountRecord, Environment, ScanOutcome};
use crate::db::paths::app_data_dir;
use crate::errors::AppError;

fn db_path() -> Result<std::path::PathBuf, AppError> {
    Ok(app_data_dir()?.join("db").join("cloudsaw.db"))
}

fn open() -> Result<Connection, AccountsError> {
    let path = db_path().map_err(|e| AccountsError::Db(e.to_string()))?;
    Connection::open(&path).map_err(AccountsError::from)
}

/// Insert a fresh row. The caller has already validated inputs and verified
/// the AWS account ID via STS. Duplicate detection (label OR aws_account_id)
/// happens here as a single transaction so two concurrent adds can't both
/// "win".
pub fn insert(record: &AccountRecord) -> Result<Account, AccountsError> {
    let mut conn = open()?;
    let tx = conn.transaction()?;

    let dup_id: Option<String> = tx
        .query_row(
            "SELECT aws_account_id FROM accounts WHERE aws_account_id = ?1",
            params![record.aws_account_id],
            |r| r.get::<_, String>(0),
        )
        .optional()?;
    if dup_id.is_some() {
        return Err(AccountsError::DuplicateAwsAccountId);
    }

    let dup_label: Option<String> = tx
        .query_row(
            "SELECT label FROM accounts WHERE label = ?1",
            params![record.label],
            |r| r.get::<_, String>(0),
        )
        .optional()?;
    if dup_label.is_some() {
        return Err(AccountsError::DuplicateLabel);
    }

    let now = Utc::now().to_rfc3339();
    tx.execute(
        "INSERT INTO accounts (
            aws_account_id, label, profile_name, environment,
            role_provisioned, role_provisioned_at,
            last_scan_at, last_scan_status,
            created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, 0, NULL, NULL, NULL, ?5, ?5)",
        params![
            record.aws_account_id,
            record.label,
            record.profile_name,
            record.environment.as_str(),
            now,
        ],
    )?;
    tx.commit()?;

    get(&record.aws_account_id)
}

/// Update mutable fields on an existing row. Caller has validated inputs and,
/// if the profile changed, verified the new profile still resolves to the
/// same AWS account ID (otherwise `AccountsError::AwsAccountIdMismatch` is
/// raised before we get here).
pub fn update_fields(
    aws_account_id: &str,
    label: &str,
    profile_name: &str,
    environment: Environment,
) -> Result<Account, AccountsError> {
    let mut conn = open()?;
    let tx = conn.transaction()?;

    // Confirm the row exists.
    let exists: Option<i64> = tx
        .query_row(
            "SELECT 1 FROM accounts WHERE aws_account_id = ?1",
            params![aws_account_id],
            |r| r.get(0),
        )
        .optional()?;
    if exists.is_none() {
        return Err(AccountsError::NotFound);
    }

    // Reject label collisions against OTHER rows (same row keeping its label
    // is fine).
    let dup_label: Option<String> = tx
        .query_row(
            "SELECT aws_account_id FROM accounts WHERE label = ?1 AND aws_account_id != ?2",
            params![label, aws_account_id],
            |r| r.get(0),
        )
        .optional()?;
    if dup_label.is_some() {
        return Err(AccountsError::DuplicateLabel);
    }

    let now = Utc::now().to_rfc3339();
    tx.execute(
        "UPDATE accounts
            SET label = ?1, profile_name = ?2, environment = ?3, updated_at = ?4
          WHERE aws_account_id = ?5",
        params![
            label,
            profile_name,
            environment.as_str(),
            now,
            aws_account_id
        ],
    )?;
    tx.commit()?;

    get(aws_account_id)
}

/// Delete a row and clear `active_account` if it pointed at this row, in a
/// single transaction. The Edge-Case rule "active account removed → app
/// clears the active selection" is honored at the storage layer so any
/// caller (IPC, tests, future onboarding) gets the same behavior.
///
/// Returns `true` if the removed row had been the active account, so the
/// caller can re-render the UI prompt accordingly.
pub fn delete(aws_account_id: &str) -> Result<bool, AccountsError> {
    let mut conn = open()?;
    let tx = conn.transaction()?;

    let active: Option<String> = tx.query_row(
        "SELECT aws_account_id FROM active_account WHERE id = 1",
        [],
        |r| r.get::<_, Option<String>>(0),
    )?;

    let was_active = active.as_deref() == Some(aws_account_id);

    let deleted = tx.execute(
        "DELETE FROM accounts WHERE aws_account_id = ?1",
        params![aws_account_id],
    )?;
    if deleted == 0 {
        return Err(AccountsError::NotFound);
    }

    if was_active {
        let now = Utc::now().to_rfc3339();
        tx.execute(
            "UPDATE active_account SET aws_account_id = NULL, updated_at = ?1 WHERE id = 1",
            params![now],
        )?;
    }

    tx.commit()?;
    Ok(was_active)
}

pub fn list() -> Result<Vec<Account>, AccountsError> {
    let conn = open()?;
    let mut stmt = conn.prepare(
        "SELECT aws_account_id, label, profile_name, environment,
                role_provisioned, role_provisioned_at,
                last_scan_at, last_scan_status,
                created_at, updated_at
           FROM accounts
       ORDER BY label ASC",
    )?;
    let rows = stmt.query_map([], row_to_account)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn get(aws_account_id: &str) -> Result<Account, AccountsError> {
    let conn = open()?;
    let row = conn
        .query_row(
            "SELECT aws_account_id, label, profile_name, environment,
                    role_provisioned, role_provisioned_at,
                    last_scan_at, last_scan_status,
                    created_at, updated_at
               FROM accounts
              WHERE aws_account_id = ?1",
            params![aws_account_id],
            row_to_account,
        )
        .optional()?
        .ok_or(AccountsError::NotFound)?;
    Ok(row)
}

pub fn get_active() -> Result<Option<String>, AccountsError> {
    let conn = open()?;
    let row: Option<Option<String>> = conn
        .query_row(
            "SELECT aws_account_id FROM active_account WHERE id = 1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(row.flatten())
}

/// Set the active account. The string must reference an existing row OR be
/// `None` (clears the active selection). Both writes happen in one
/// transaction to keep the singleton in a defined state.
pub fn set_active(aws_account_id: Option<&str>) -> Result<(), AccountsError> {
    let mut conn = open()?;
    let tx = conn.transaction()?;

    if let Some(id) = aws_account_id {
        let exists: Option<i64> = tx
            .query_row(
                "SELECT 1 FROM accounts WHERE aws_account_id = ?1",
                params![id],
                |r| r.get(0),
            )
            .optional()?;
        if exists.is_none() {
            return Err(AccountsError::NotFound);
        }
    }

    let now = Utc::now().to_rfc3339();
    tx.execute(
        "UPDATE active_account SET aws_account_id = ?1, updated_at = ?2 WHERE id = 1",
        params![aws_account_id, now],
    )?;
    tx.commit()?;
    Ok(())
}

pub fn get_reveal_full_ids() -> Result<bool, AccountsError> {
    let conn = open()?;
    let row: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'accounts.reveal_full_ids'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(matches!(row.as_deref(), Some("1")))
}

pub fn set_reveal_full_ids(reveal: bool) -> Result<(), AccountsError> {
    let conn = open()?;
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO settings (key, value, updated_at) VALUES ('accounts.reveal_full_ids', ?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![if reveal { "1" } else { "0" }, now],
    )?;
    Ok(())
}

fn row_to_account(row: &rusqlite::Row<'_>) -> rusqlite::Result<Account> {
    let aws_account_id: String = row.get(0)?;
    let label: String = row.get(1)?;
    let profile_name: String = row.get(2)?;
    let environment_str: String = row.get(3)?;
    let role_provisioned: i64 = row.get(4)?;
    let role_provisioned_at: Option<String> = row.get(5)?;
    let last_scan_at: Option<String> = row.get(6)?;
    let last_scan_status: Option<String> = row.get(7)?;
    let created_at: String = row.get(8)?;
    let updated_at: String = row.get(9)?;

    Ok(Account {
        aws_account_id,
        label,
        profile_name,
        environment: Environment::from_storage(&environment_str),
        role_provisioned: role_provisioned != 0,
        role_provisioned_at: parse_ts(role_provisioned_at)?,
        last_scan_at: parse_ts(last_scan_at)?,
        last_scan_status: last_scan_status.map(ScanOutcome::from_storage),
        created_at: parse_required_ts(created_at)?,
        updated_at: parse_required_ts(updated_at)?,
    })
}

fn parse_ts(s: Option<String>) -> rusqlite::Result<Option<DateTime<Utc>>> {
    match s {
        None => Ok(None),
        Some(raw) => DateTime::parse_from_rfc3339(&raw)
            .map(|dt| Some(dt.with_timezone(&Utc)))
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            }),
    }
}

fn parse_required_ts(raw: String) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&raw)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })
}
