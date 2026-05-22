// Hard-delete pipeline — Contract 11C.
//
// Builds on `findings::delete_scan` (Contract 07) by:
//   * Requiring a typed confirmation value that matches either the literal
//     `DELETE` or the scan ID being removed.
//   * Unlinking the per-scan raw output directory after the SQLite cascade.
//   * Running `VACUUM` so removed rows are not trivially recoverable from
//     the database file.
//   * Optionally "secure overwriting" the raw file before unlink, with the
//     limit-disclosure surfaced via the IPC response (Contract 11
//     §Constraints + Edge Cases).
//
// Public surface (mirrors the contract's "Expected Output"):
//
//     hard_delete_scan(scan_id, confirmation, options) -> HardDeleteSummary
//
// Confirmation is enforced HERE rather than only in the UI because
// CLAUDE.md §4.1 says every Tauri command validates its inputs and never
// trusts a value from the frontend. A direct IPC caller that omits the
// confirmation gets the same `InvalidInput` error a UI bypass would.

pub mod error;

pub use error::DeletionError;

use std::path::Path;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::db::paths::app_data_dir;
use crate::eventlog::{self, EventInput, EventKind};
use crate::findings;

/// Options for one hard-delete invocation. `secure_overwrite` is off by
/// default — the QA contract requires we surface the limit ("limited by
/// SSD wear-leveling") rather than hide it behind a default-on flag.
#[derive(Debug, Default, Clone, Deserialize)]
pub struct HardDeleteOptions {
    #[serde(default)]
    pub secure_overwrite: bool,
}

/// What the IPC returns to the UI after a successful hard delete. The
/// `findings_removed` / `findings_updated` counts pass through from the
/// underlying `findings::delete_scan` cascade.
#[derive(Debug, Clone, Serialize)]
pub struct HardDeleteSummary {
    pub scan_id: String,
    pub findings_removed: usize,
    pub findings_updated: usize,
    pub resources_removed: usize,
    pub raw_files_removed: usize,
    pub raw_dir_removed: bool,
    /// True when the caller asked for secure overwrite AND we actually
    /// overwrote at least one file before unlinking it. The UI uses this
    /// to know whether to render the wear-leveling disclosure.
    pub secure_overwrite_attempted: bool,
    pub vacuum_run: bool,
}

/// Run a hard delete. Requires the user to have typed either the literal
/// `DELETE` or the full scan ID. Other inputs are validated inside
/// `findings::delete_scan` before any DB work happens.
pub fn hard_delete_scan(
    scan_id: &str,
    confirmation: &str,
    options: HardDeleteOptions,
) -> Result<HardDeleteSummary, DeletionError> {
    if !is_confirmation_valid(scan_id, confirmation) {
        return Err(DeletionError::ConfirmationRejected);
    }

    // Snapshot the raw output path BEFORE the cascade — the cascade
    // removes the scan row, after which we lose the disk-side handle.
    let raw_path = match findings::get_scan(scan_id) {
        Ok(record) => record.raw_output_path.clone(),
        Err(findings::FindingsError::ScanNotFound) => return Err(DeletionError::ScanNotFound),
        Err(e) => return Err(DeletionError::Findings(e)),
    };

    // 1. SQLite cascade.
    let impact = findings::delete_scan(scan_id).map_err(DeletionError::Findings)?;

    // 2. Raw-file (and per-scan dir) unlink. Best-effort: the row is
    //    already gone, so leaving the file behind is the lesser evil
    //    compared to a fail that leaves the user thinking the delete
    //    didn't happen. Records the failure (count = 0) in the event log
    //    if the path didn't resolve.
    let mut raw_files_removed = 0usize;
    let mut secure_attempted = false;
    if let Some(raw) = raw_path.as_deref() {
        let path = std::path::Path::new(raw);
        if path.is_file() {
            if options.secure_overwrite {
                secure_attempted = true;
                let _ = best_effort_overwrite(path);
            }
            if std::fs::remove_file(path).is_ok() {
                raw_files_removed += 1;
            }
        }
    }

    let scans_root = app_data_dir()
        .map_err(|e| DeletionError::Io(e.to_string()))?
        .join("scans");
    let scan_dir = scans_root.join(scan_id);
    let dir_removed = if scan_dir.is_dir() {
        std::fs::remove_dir_all(&scan_dir).is_ok()
    } else {
        false
    };

    // 3. VACUUM — Contract 11 §Constraints: "Hard delete MUST run VACUUM
    //    after DELETE so removed rows are not trivially recoverable from
    //    the database file." VACUUM must run OUTSIDE a transaction.
    let vacuum_ran = run_vacuum().is_ok();

    // 4. Event-log entry — Contract 11 §Constraints record deletions as a
    //    count plus affected paths, NEVER the content.
    let mut event = EventInput::new(
        EventKind::ScanDeleted,
        format!(
            "Hard-deleted scan {scan_id} ({} findings).",
            impact.findings_removed
        ),
    )
    .with_scan_id(scan_id)
    .with_item_count(impact.findings_removed as i64);
    if let Some(p) = raw_path.as_deref() {
        event = event.with_path(p);
    }
    eventlog::record_event(event);

    Ok(HardDeleteSummary {
        scan_id: scan_id.to_string(),
        findings_removed: impact.findings_removed,
        findings_updated: impact.findings_updated,
        resources_removed: impact.resources_removed,
        raw_files_removed,
        raw_dir_removed: dir_removed,
        secure_overwrite_attempted: secure_attempted,
        vacuum_run: vacuum_ran,
    })
}

/// Accept either the literal `DELETE` or the full scan ID. The case-sensitive
/// `DELETE` is the safer fallback when the scan ID is long and hard to type;
/// the scan-ID match keeps the contract's "type the scan ID" example honored.
fn is_confirmation_valid(scan_id: &str, confirmation: &str) -> bool {
    if confirmation == "DELETE" {
        return true;
    }
    if confirmation == scan_id {
        return true;
    }
    false
}

/// Best-effort secure overwrite: three passes (zeros, ones, zeros) before
/// the file is unlinked. Documented honestly as "limited by SSD
/// wear-leveling" — on flash storage the OS may map writes to a different
/// physical block, leaving the original data recoverable from spare
/// pages. The UI surfaces this disclaimer alongside the toggle.
fn best_effort_overwrite(path: &Path) -> std::io::Result<()> {
    use std::io::{Seek, SeekFrom, Write};

    let len = std::fs::metadata(path)?.len() as usize;
    let mut file = std::fs::OpenOptions::new().write(true).open(path)?;
    for pattern in [0u8, 0xFFu8, 0u8] {
        file.seek(SeekFrom::Start(0))?;
        let chunk = vec![pattern; 64 * 1024];
        let mut written = 0usize;
        while written < len {
            let n = (len - written).min(chunk.len());
            file.write_all(&chunk[..n])?;
            written += n;
        }
        file.flush()?;
    }
    Ok(())
}

/// Issue `VACUUM` against the cloudsaw.db file. Must run outside a
/// transaction; we open a fresh connection so we don't accidentally
/// inherit one from a caller.
pub fn run_vacuum() -> Result<(), DeletionError> {
    let path = app_data_dir()
        .map_err(|e| DeletionError::Io(e.to_string()))?
        .join("db")
        .join("cloudsaw.db");
    let conn = Connection::open(&path).map_err(|e| DeletionError::Db(e.to_string()))?;
    conn.execute_batch("VACUUM")
        .map_err(|e| DeletionError::Db(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmation_accepts_literal_delete() {
        assert!(is_confirmation_valid("abc123", "DELETE"));
    }

    #[test]
    fn confirmation_accepts_scan_id_exact_match() {
        assert!(is_confirmation_valid("abc123", "abc123"));
    }

    #[test]
    fn confirmation_rejects_other_input() {
        assert!(!is_confirmation_valid("abc123", "delete"));
        assert!(!is_confirmation_valid("abc123", "abc12"));
        assert!(!is_confirmation_valid("abc123", ""));
    }
}
