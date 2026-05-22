// Findings parser & store — Contract 07.
//
// CloudSaw's scanner orchestrator (Contract 06) drops a `raw-scout.json`
// file per scan. This module turns that file into a normalized,
// account-partitioned record in SQLite, ready for the Dashboard / Drift /
// Reports surfaces built by Contracts 09 and 15.
//
// Public surface (mirrors Contract 07 §Expected Output):
//
//     parse_and_store(scan_id)        -> ParseSummary
//     list_findings(scan_id, filter)  -> Vec<Finding>
//     get_finding(finding_id)         -> FindingDetail
//     list_scans(aws_account_id)      -> Vec<ScanRecord>
//     get_scan(scan_id)               -> ScanRecord
//     delete_scan(scan_id)            -> DeleteScanImpact
//
// What this module DOES NOT do (and never will):
//   - Read or write credentials. Findings are configuration-shaped data;
//     credentials live only in the OS keychain (CLAUDE.md §4.3).
//   - Open the network. The parser only reads the local `raw-scout.json`
//     dropped on disk by Contract 06.
//   - Concatenate SQL strings. Every statement uses parameterized binds
//     (CLAUDE.md §4.5); the one dynamically-shaped fragment is a
//     placeholder list whose values are still bound, never interpolated.
//   - Transmit any of this data anywhere. CLAUDE.md §5 hard-DO-NOT.

pub mod error;
pub mod parser;
pub mod storage;
pub mod types;

pub use error::FindingsError;
pub use types::{
    DeleteScanImpact, Finding, FindingDetail, FindingResource, FindingStatus, FindingsFilter,
    ParseSummary, Severity,
};

use crate::accounts;
use crate::scanner::types::ScanRecord;

/// Parse the ScoutSuite output for `scan_id` and persist the normalized
/// findings, resources, and per-scan observations.
///
/// Idempotency: re-running this with the same `scan_id` yields zero net
/// change. Atomicity: malformed input never produces partial writes — the
/// transaction is rolled back and the scan row is flipped to `failed`
/// (Contract 07 §Edge Cases).
pub fn parse_and_store(scan_id: &str) -> Result<ParseSummary, FindingsError> {
    validate_scan_id(scan_id)?;

    let scan = storage::get_scan_row(scan_id)?;
    let raw_path = scan
        .raw_output_path
        .as_ref()
        .ok_or(FindingsError::NoRawOutput)?
        .clone();

    let path = std::path::Path::new(&raw_path);
    if !path.is_file() {
        return Err(FindingsError::RawOutputMissing);
    }

    let bytes = std::fs::read(path).map_err(|e| FindingsError::Io(e.to_string()))?;
    let json: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(e) => {
            // Mark the scan failed so the UI surfaces this rather than
            // showing "successfully parsed: 0 findings". The mark itself
            // can fail (db locked, scan deleted between calls) — ignore
            // the secondary error so the caller still gets the primary
            // parse_malformed signal.
            let _ = storage::mark_scan_failed(scan_id, "parse_malformed_json");
            return Err(FindingsError::ParseMalformed(e.to_string()));
        }
    };

    let parsed = parser::parse_scoutsuite(&json);

    // Defense in depth: ScoutSuite echoes the account_id it scanned, and
    // we already know which account this scan belongs to. A mismatch is
    // either a corrupt file or a swapped scan_id — refuse to store
    // either way (CLAUDE.md §4.1: account_id is the partitioning key,
    // never inferred from untrusted input).
    if let Some(echoed) = parsed.account_id.as_deref() {
        if echoed != scan.aws_account_id {
            let _ = storage::mark_scan_failed(scan_id, "parse_account_mismatch");
            return Err(FindingsError::AccountMismatch);
        }
    }

    storage::apply_parsed(&scan, &parsed)
}

/// Findings observed in a single scan, optionally filtered by severity,
/// service, or status. Always partitioned by the scan's account_id.
pub fn list_findings(scan_id: &str, filter: FindingsFilter) -> Result<Vec<Finding>, FindingsError> {
    validate_scan_id(scan_id)?;
    storage::list_findings_for_scan(scan_id, &filter)
}

/// Full detail (finding row + resources) by stable finding_id.
pub fn get_finding(finding_id: &str) -> Result<FindingDetail, FindingsError> {
    if finding_id.len() != 64 || !finding_id.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(FindingsError::InvalidInput("finding_id"));
    }
    storage::get_finding_detail(finding_id)
}

/// All scans for an account, newest first. Drives the History tab.
pub fn list_scans(aws_account_id: &str) -> Result<Vec<ScanRecord>, FindingsError> {
    validate_account_id(aws_account_id)?;
    storage::list_scans_for_account(aws_account_id, 200)
}

/// Single scan by ID. Returned shape mirrors `scanner::scan_status` — the
/// findings module re-exposes it so the UI can fetch scan+findings via
/// one consistent IPC namespace.
pub fn get_scan(scan_id: &str) -> Result<ScanRecord, FindingsError> {
    validate_scan_id(scan_id)?;
    storage::get_scan_row(scan_id)
}

/// Cascade-delete a scan. Removes the scan row, its `scan_findings`
/// rows, and any finding/resource whose only observation was this scan.
/// Findings still observed by other scans are retained with their
/// last_seen pointers recomputed.
///
/// Confirmation is the UI's responsibility (CLAUDE.md §5: destructive
/// actions require explicit typed confirmation). This function never
/// asks; it just executes.
pub fn delete_scan(scan_id: &str) -> Result<DeleteScanImpact, FindingsError> {
    validate_scan_id(scan_id)?;
    storage::delete_scan_cascade(scan_id)
}

/// Best-effort EXPLAIN QUERY PLAN inspector for the severity-filtered
/// list query. Used by the QA contract to assert the query is
/// index-backed (Contract 07 §Acceptance Criteria + QA §Security Check).
pub fn explain_severity_filtered(
    scan_id: &str,
    severity: Severity,
) -> Result<Vec<String>, FindingsError> {
    validate_scan_id(scan_id)?;
    storage::explain_severity_filtered_list(scan_id, severity)
}

fn validate_scan_id(scan_id: &str) -> Result<(), FindingsError> {
    if scan_id.is_empty() || scan_id.len() > 128 {
        return Err(FindingsError::InvalidInput("scan_id"));
    }
    if !scan_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(FindingsError::InvalidInput("scan_id"));
    }
    Ok(())
}

fn validate_account_id(id: &str) -> Result<(), FindingsError> {
    if id.len() == 12 && id.chars().all(|c| c.is_ascii_digit()) {
        Ok(())
    } else {
        Err(FindingsError::InvalidInput("aws_account_id"))
    }
}

/// Mask a finding's account_id for logging — same masking rule as
/// `accounts::mask_for_logs`, re-exposed so callers don't have to reach
/// into the accounts module for it.
pub fn mask_account_for_logs(aws_account_id: &str) -> String {
    accounts::mask_for_logs(aws_account_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_scan_id_accepts_hex_ids_and_rejects_garbage() {
        assert!(validate_scan_id("abcdef0123456789").is_ok());
        assert!(validate_scan_id("not-empty-but-with-hyphens").is_ok());
        assert!(validate_scan_id("").is_err());
        assert!(validate_scan_id("contains spaces").is_err());
        assert!(validate_scan_id("contains/slash").is_err());
    }

    #[test]
    fn validate_account_id_requires_12_digits() {
        assert!(validate_account_id("111122223333").is_ok());
        assert!(validate_account_id("11112222333").is_err());
        assert!(validate_account_id("11112222333a").is_err());
    }
}
