// Settings storage for the report exporter (Contract 15 §Expected
// Output). Three rows in the existing `settings` table — non-secret.
//
// `enabled` is the user-facing on/off toggle. `folder` is the
// absolute directory path the user picked through the OS folder
// picker — it MUST be an existing directory by the time it lands
// here (the picker enforces that). `mask_account_ids_default` flips
// the default disclosure mode for new exports; the export flow still
// lets the user opt in to full IDs on a per-export basis.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::error::ReportsError;
use crate::db::paths::app_data_dir;

const KEY_FOLDER: &str = "report_auto_export_folder";
const KEY_ENABLED: &str = "report_auto_export_enabled";
const KEY_MASK_DEFAULT: &str = "report_mask_account_ids_default";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportSettings {
    pub auto_export_enabled: bool,
    pub auto_export_folder: Option<String>,
    pub mask_account_ids_default: bool,
}

fn db_path() -> Result<std::path::PathBuf, ReportsError> {
    Ok(app_data_dir()
        .map_err(|e| ReportsError::Io(e.to_string()))?
        .join("db")
        .join("cloudsaw.db"))
}

fn open() -> Result<Connection, ReportsError> {
    Connection::open(db_path()?).map_err(ReportsError::from)
}

fn read_value(conn: &Connection, key: &str) -> Result<String, ReportsError> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |r| r.get(0),
        )
        .optional()?;
    Ok(raw.unwrap_or_default())
}

fn write_value(key: &str, value: &str) -> Result<(), ReportsError> {
    let conn = open()?;
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO settings (key, value, updated_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value,
                                        updated_at = excluded.updated_at",
        params![key, value, now],
    )?;
    Ok(())
}

pub fn read() -> Result<ReportSettings, ReportsError> {
    let conn = open()?;
    let folder_raw = read_value(&conn, KEY_FOLDER)?;
    let enabled_raw = read_value(&conn, KEY_ENABLED)?;
    let mask_raw = read_value(&conn, KEY_MASK_DEFAULT)?;
    Ok(ReportSettings {
        auto_export_enabled: enabled_raw == "1",
        auto_export_folder: if folder_raw.trim().is_empty() {
            None
        } else {
            Some(folder_raw)
        },
        mask_account_ids_default: !(mask_raw == "0"),
    })
}

pub fn write(settings: &ReportSettings) -> Result<(), ReportsError> {
    if let Some(folder) = settings.auto_export_folder.as_deref() {
        if folder.len() > 4096 {
            return Err(ReportsError::InvalidInput("auto_export_folder"));
        }
    }
    write_value(
        KEY_FOLDER,
        settings.auto_export_folder.as_deref().unwrap_or(""),
    )?;
    write_value(
        KEY_ENABLED,
        if settings.auto_export_enabled {
            "1"
        } else {
            "0"
        },
    )?;
    write_value(
        KEY_MASK_DEFAULT,
        if settings.mask_account_ids_default {
            "1"
        } else {
            "0"
        },
    )?;
    Ok(())
}
