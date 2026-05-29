// SQLite-backed read/write for `findings`, `scan_findings`, and
// `finding_resources`.
//
// Every public function opens its own connection. Findings ops happen on
// user cadence (parse-after-scan, list-on-tab-open, delete-on-confirm) —
// the simpler per-call connection mirrors `accounts::storage` and
// `scanner::storage`.
//
// CLAUDE.md §4.5: every SQL statement here uses parameterized queries. No
// string interpolation, anywhere. Severity / status enums are converted
// via their `as_str()` form before binding so the CHECK constraints in
// migration 0006 never see anything they don't recognize.

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Transaction};

use super::error::FindingsError;
use super::parser::{finding_id_for, ParsedFinding, ParsedScoutOutput};
use super::types::{
    DeleteScanImpact, Finding, FindingDetail, FindingResource, FindingStatus, FindingsFilter,
    ParseSummary, Severity,
};
use crate::db::paths::app_data_dir;
use crate::scanner::types::{ScanRecord, ScanStatus};

fn db_path() -> Result<std::path::PathBuf, FindingsError> {
    Ok(app_data_dir()
        .map_err(|e| FindingsError::Io(e.to_string()))?
        .join("db")
        .join("cloudsaw.db"))
}

fn open() -> Result<Connection, FindingsError> {
    Connection::open(db_path()?).map_err(FindingsError::from)
}

/// Look up the scan row by ID. Returns the fields the parser cares about
/// (`aws_account_id`, `started_at`, `raw_output_path`).
///
/// Defined here (rather than reaching into `scanner::storage`) so the
/// findings module owns a single read of the scans table and so we can
/// borrow the transaction in `apply_parsed` without crossing module
/// boundaries.
pub fn get_scan_row(scan_id: &str) -> Result<ScanRecord, FindingsError> {
    let conn = open()?;
    let row = conn
        .query_row(
            "SELECT scan_id, aws_account_id, status, started_at, finished_at,
                    failure_code, warning_code, warning_detail, raw_output_path,
                    role_session_name, truncated
               FROM scans
              WHERE scan_id = ?1",
            params![scan_id],
            row_to_scan_record,
        )
        .optional()?
        .ok_or(FindingsError::ScanNotFound)?;
    Ok(row)
}

/// List scans for an account, newest first. Drives the History tab.
/// Reuses the scans table indexed by `(aws_account_id, started_at DESC)`
/// from migration 0005.
pub fn list_scans_for_account(
    aws_account_id: &str,
    limit: i64,
) -> Result<Vec<ScanRecord>, FindingsError> {
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
    let rows = stmt.query_map(params![aws_account_id, limit], row_to_scan_record)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Mark a scan as failed with a stable code. Used by `parse_and_store`
/// when the raw JSON is malformed — Contract 07 §Edge Cases.
///
/// Mirrors the lighter-weight path of `scanner::storage::record_failed`
/// without touching the accounts mirror (which the original scan-failure
/// path owns).
pub fn mark_scan_failed(scan_id: &str, failure_code: &str) -> Result<(), FindingsError> {
    let conn = open()?;
    let now = Utc::now().to_rfc3339();
    let affected = conn.execute(
        "UPDATE scans
            SET status = ?1,
                finished_at = COALESCE(finished_at, ?2),
                failure_code = ?3
          WHERE scan_id = ?4",
        params![ScanStatus::Failed.as_str(), now, failure_code, scan_id],
    )?;
    if affected == 0 {
        return Err(FindingsError::ScanNotFound);
    }
    Ok(())
}

/// Apply the parsed model to the database transactionally. Returns the
/// per-scan summary counters.
///
/// The contract requires:
///   * Idempotency: re-running on the same scan_id produces zero net
///     change.
///   * Atomicity: malformed input does not produce partial writes.
///   * Time-series bookkeeping: first_seen_at / last_seen_at update on
///     repeat observation; status flips to `resolved` when a later scan
///     covers the service but no longer reports the finding.
///
/// The implementation is a single transaction with three phases:
///   1. Upsert per-finding rows + scan_findings join + per-resource rows.
///   2. Resolution sweep: for findings in this account whose service was
///      scanned but rule_key was not present, mark resolved (if not
///      already resolved by a later scan).
///   3. Commit.
pub fn apply_parsed(
    scan: &ScanRecord,
    parsed: &ParsedScoutOutput,
) -> Result<ParseSummary, FindingsError> {
    let mut conn = open()?;
    let tx = conn.transaction()?;
    let scan_ts = scan.started_at.to_rfc3339();
    let account_id = &scan.aws_account_id;
    let scan_id = &scan.scan_id;

    let mut summary = ParseSummary {
        scan_id: scan_id.clone(),
        aws_account_id: account_id.clone(),
        findings_total: parsed.findings.len(),
        findings_inserted: 0,
        findings_updated: 0,
        findings_resolved: 0,
        resources_inserted: 0,
        resources_updated: 0,
        unknown_severity_count: parsed.unknown_severity_count,
        unknown_type_count: parsed.unknown_type_count,
    };

    // Phase 1: upsert findings + resources + scan_findings.
    for finding in &parsed.findings {
        upsert_finding(&tx, account_id, scan_id, &scan_ts, finding, &mut summary)?;
    }

    // Phase 2: resolution sweep. Only findings whose `service` was in the
    // current scan's `services_scanned` set are eligible — otherwise the
    // scan can't tell us anything about them.
    if !parsed.services_scanned.is_empty() {
        let observed_ids: std::collections::HashSet<String> = parsed
            .findings
            .iter()
            .map(|f| finding_id_for(account_id, &f.rule_key))
            .collect();

        let candidates: Vec<(String, String, String)> = {
            // Build the IN-clause for services dynamically. Each value is
            // bound as a parameter, never interpolated; the IN list shape
            // is the only thing computed from the input, and its size is
            // bounded by the number of distinct services ScoutSuite
            // emits per scan (well under 100).
            let placeholders: Vec<String> = (0..parsed.services_scanned.len())
                .map(|i| format!("?{}", i + 2))
                .collect();
            let sql = format!(
                "SELECT finding_id, service, last_seen_at
                   FROM findings
                  WHERE aws_account_id = ?1
                    AND status = 'open'
                    AND service IN ({})",
                placeholders.join(", ")
            );
            let mut stmt = tx.prepare(&sql)?;
            let mut params_vec: Vec<&dyn rusqlite::ToSql> = Vec::new();
            params_vec.push(account_id);
            for s in &parsed.services_scanned {
                params_vec.push(s);
            }
            let rows = stmt.query_map(params_vec.as_slice(), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?;
            let mut out = Vec::new();
            for r in rows {
                out.push(r?);
            }
            out
        };

        for (finding_id, _service, existing_last_seen) in candidates {
            if observed_ids.contains(&finding_id) {
                continue;
            }
            // Only resolve when this scan is at least as recent as the
            // finding's last_seen_at. Otherwise we'd be back-dating a
            // resolution from an older scan we just re-parsed.
            if scan_ts < existing_last_seen {
                continue;
            }
            let updated = tx.execute(
                "UPDATE findings
                    SET status = 'resolved',
                        resolved_at = ?1,
                        resolved_in_scan_id = ?2
                  WHERE finding_id = ?3
                    AND status = 'open'",
                params![scan_ts, scan_id, finding_id],
            )?;
            if updated > 0 {
                summary.findings_resolved += 1;
            }
        }
    }

    tx.commit()?;

    // PR #70 — record the auto-resolution sweep in the activity log
    // so the user can see, after the fact, that "scan X resolved N
    // prior findings for account Y". Best-effort; an event-log write
    // failure must never roll back the findings transaction.
    if summary.findings_resolved > 0 {
        use crate::eventlog::{record_event, EventInput, EventKind};
        record_event(
            EventInput::new(
                EventKind::FindingsAutoResolved,
                format!(
                    "Scan {scan} auto-resolved {n} prior finding(s) for {acct}.",
                    scan = scan_id,
                    n = summary.findings_resolved,
                    acct = crate::accounts::mask_for_logs(account_id),
                ),
            )
            .with_scan_id(scan_id)
            .with_account(account_id)
            .with_item_count(summary.findings_resolved as i64),
        );
    }

    Ok(summary)
}

/// One finding's upsert. Splits the INSERT and UPDATE paths so the SQL is
/// readable and so we can return whether the row was newly created (drives
/// summary counters and idempotency: a second parse hits the UPDATE path
/// and writes the same values).
#[allow(clippy::too_many_arguments)]
fn upsert_finding(
    tx: &Transaction<'_>,
    account_id: &str,
    scan_id: &str,
    scan_ts: &str,
    finding: &ParsedFinding,
    summary: &mut ParseSummary,
) -> Result<(), FindingsError> {
    let finding_id = finding_id_for(account_id, &finding.rule_key);

    let existing: Option<(String, String)> = tx
        .query_row(
            "SELECT last_seen_at, status FROM findings WHERE finding_id = ?1",
            params![finding_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;

    match existing {
        None => {
            tx.execute(
                "INSERT INTO findings (
                    finding_id, aws_account_id, rule_key, raw_type, service,
                    severity, description, rationale, dashboard_name,
                    resource_path_pattern, checked_items, flagged_items,
                    status, first_seen_at, last_seen_at,
                    first_seen_scan_id, last_seen_scan_id,
                    resolved_at, resolved_in_scan_id
                 ) VALUES (
                    ?1, ?2, ?3, ?4, ?5,
                    ?6, ?7, ?8, ?9,
                    ?10, ?11, ?12,
                    'open', ?13, ?13,
                    ?14, ?14,
                    NULL, NULL
                 )",
                params![
                    finding_id,
                    account_id,
                    finding.rule_key,
                    finding.raw_type,
                    finding.service,
                    finding.severity.as_str(),
                    finding.description,
                    finding.rationale,
                    finding.dashboard_name,
                    finding.resource_path_pattern,
                    finding.checked_items,
                    finding.flagged_items,
                    scan_ts,
                    scan_id,
                ],
            )?;
            summary.findings_inserted += 1;
        }
        Some((last_seen_at, _status)) => {
            // Only update when this scan is at least as recent as the
            // existing row. Older scans being re-parsed never overwrite
            // newer data — this is what makes re-parsing safe (Contract 07
            // §Constraints: pure parser, identical input → identical
            // stored data).
            if scan_ts >= last_seen_at.as_str() {
                let affected = tx.execute(
                    "UPDATE findings
                        SET raw_type             = ?1,
                            service              = ?2,
                            severity             = ?3,
                            description          = ?4,
                            rationale            = ?5,
                            dashboard_name       = ?6,
                            resource_path_pattern = ?7,
                            checked_items        = ?8,
                            flagged_items        = ?9,
                            status               = 'open',
                            last_seen_at         = ?10,
                            last_seen_scan_id    = ?11,
                            resolved_at          = NULL,
                            resolved_in_scan_id  = NULL
                      WHERE finding_id = ?12",
                    params![
                        finding.raw_type,
                        finding.service,
                        finding.severity.as_str(),
                        finding.description,
                        finding.rationale,
                        finding.dashboard_name,
                        finding.resource_path_pattern,
                        finding.checked_items,
                        finding.flagged_items,
                        scan_ts,
                        scan_id,
                        finding_id,
                    ],
                )?;
                if affected > 0 {
                    summary.findings_updated += 1;
                }
            }
        }
    }

    // Join row for this (scan, finding). INSERT OR IGNORE keeps re-parse
    // idempotent — we never duplicate the observation.
    tx.execute(
        "INSERT OR IGNORE INTO scan_findings (scan_id, finding_id, aws_account_id, observed_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![scan_id, finding_id, account_id, scan_ts],
    )?;

    // Resources for this finding.
    for resource in &finding.resources {
        let resource_existing: Option<String> = tx
            .query_row(
                "SELECT last_seen_at FROM finding_resources
                  WHERE finding_id = ?1 AND resource_path = ?2",
                params![finding_id, resource.resource_path],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        match resource_existing {
            None => {
                tx.execute(
                    "INSERT INTO finding_resources (
                        finding_id, aws_account_id, resource_path, invalid,
                        first_seen_at, last_seen_at,
                        first_seen_scan_id, last_seen_scan_id
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6, ?6)",
                    params![
                        finding_id,
                        account_id,
                        resource.resource_path,
                        i64::from(resource.invalid),
                        scan_ts,
                        scan_id,
                    ],
                )?;
                summary.resources_inserted += 1;
            }
            Some(existing_ts) => {
                if scan_ts >= existing_ts.as_str() {
                    let affected = tx.execute(
                        "UPDATE finding_resources
                            SET invalid           = ?1,
                                last_seen_at      = ?2,
                                last_seen_scan_id = ?3
                          WHERE finding_id = ?4 AND resource_path = ?5",
                        params![
                            i64::from(resource.invalid),
                            scan_ts,
                            scan_id,
                            finding_id,
                            resource.resource_path,
                        ],
                    )?;
                    if affected > 0 {
                        summary.resources_updated += 1;
                    }
                }
            }
        }
    }
    Ok(())
}

/// List findings observed in a single scan, optionally filtered.
///
/// The query joins `scan_findings` to `findings`. The index on
/// `scan_findings(scan_id)` (the table's PRIMARY KEY) keeps the join
/// index-backed even at 50k findings (verified by EXPLAIN QUERY PLAN in
/// `tests/findings_storage_test.rs`).
pub fn list_findings_for_scan(
    scan_id: &str,
    filter: &FindingsFilter,
) -> Result<Vec<Finding>, FindingsError> {
    // Establish the account scope: every list query must filter by
    // account_id (Contract 07 §Constraints). The scan row supplies it.
    let scan = get_scan_row(scan_id)?;
    let account_id = &scan.aws_account_id;

    let conn = open()?;

    let mut sql = String::from(
        "SELECT f.finding_id, f.aws_account_id, f.rule_key, f.raw_type, f.service,
                f.severity, f.description, f.rationale, f.dashboard_name,
                f.resource_path_pattern, f.checked_items, f.flagged_items,
                f.status, f.first_seen_at, f.last_seen_at,
                f.first_seen_scan_id, f.last_seen_scan_id,
                f.resolved_at, f.resolved_in_scan_id
           FROM findings f
           JOIN scan_findings sf ON sf.finding_id = f.finding_id
          WHERE sf.scan_id = ?1
            AND f.aws_account_id = ?2",
    );
    let mut binds: Vec<Box<dyn rusqlite::ToSql>> = vec![
        Box::new(scan_id.to_string()),
        Box::new(account_id.to_string()),
    ];

    if !filter.severity.is_empty() {
        let placeholders: Vec<String> = filter
            .severity
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", binds.len() + i + 1))
            .collect();
        sql.push_str(&format!(" AND f.severity IN ({})", placeholders.join(", ")));
        for sev in &filter.severity {
            binds.push(Box::new(sev.as_str().to_string()));
        }
    }

    if let Some(service) = &filter.service {
        binds.push(Box::new(service.clone()));
        sql.push_str(&format!(" AND f.service = ?{}", binds.len()));
    }

    if let Some(status) = filter.status {
        binds.push(Box::new(status.as_str().to_string()));
        sql.push_str(&format!(" AND f.status = ?{}", binds.len()));
    }

    sql.push_str(" ORDER BY f.severity_rank_lookup, f.last_seen_at DESC");
    // We use a CASE expression for severity sort rather than adding a
    // computed column — keeps the schema small and avoids a redundant
    // index.
    sql = sql.replace(
        "f.severity_rank_lookup",
        "CASE f.severity
            WHEN 'critical' THEN 0
            WHEN 'high' THEN 1
            WHEN 'medium' THEN 2
            WHEN 'low' THEN 3
            WHEN 'informational' THEN 4
            ELSE 5
         END",
    );

    let limit = filter.limit.unwrap_or(500).clamp(1, 5000);
    let offset = filter.offset.unwrap_or(0).max(0);
    binds.push(Box::new(limit));
    sql.push_str(&format!(" LIMIT ?{}", binds.len()));
    binds.push(Box::new(offset));
    sql.push_str(&format!(" OFFSET ?{}", binds.len()));

    let mut stmt = conn.prepare(&sql)?;
    let bind_refs: Vec<&dyn rusqlite::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
    let rows = stmt.query_map(bind_refs.as_slice(), row_to_finding)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Full finding detail by finding_id, including all current resources.
pub fn get_finding_detail(finding_id: &str) -> Result<FindingDetail, FindingsError> {
    let conn = open()?;
    let finding = conn
        .query_row(
            "SELECT finding_id, aws_account_id, rule_key, raw_type, service,
                    severity, description, rationale, dashboard_name,
                    resource_path_pattern, checked_items, flagged_items,
                    status, first_seen_at, last_seen_at,
                    first_seen_scan_id, last_seen_scan_id,
                    resolved_at, resolved_in_scan_id
               FROM findings
              WHERE finding_id = ?1",
            params![finding_id],
            row_to_finding,
        )
        .optional()?
        .ok_or(FindingsError::FindingNotFound)?;

    let mut stmt = conn.prepare(
        "SELECT finding_id, aws_account_id, resource_path, invalid,
                first_seen_at, last_seen_at
           FROM finding_resources
          WHERE finding_id = ?1
       ORDER BY resource_path ASC",
    )?;
    let rows = stmt.query_map(params![finding_id], row_to_resource)?;
    let mut resources = Vec::new();
    for r in rows {
        resources.push(r?);
    }
    Ok(FindingDetail { finding, resources })
}

/// EXPLAIN QUERY PLAN for the severity-filtered list. Exposed for the QA
/// integration test that asserts the query is index-backed.
pub fn explain_severity_filtered_list(
    scan_id: &str,
    severity: Severity,
) -> Result<Vec<String>, FindingsError> {
    let scan = get_scan_row(scan_id)?;
    let conn = open()?;
    let mut stmt = conn.prepare(
        "EXPLAIN QUERY PLAN
         SELECT f.finding_id
           FROM findings f
           JOIN scan_findings sf ON sf.finding_id = f.finding_id
          WHERE sf.scan_id = ?1
            AND f.aws_account_id = ?2
            AND f.severity = ?3",
    )?;
    let rows = stmt.query_map(
        params![scan_id, scan.aws_account_id, severity.as_str()],
        |row| row.get::<_, String>(3),
    )?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Cascade-delete a scan. Removes the scan row, its scan_findings join
/// rows, and any finding/resource whose ONLY observation was this scan.
/// Findings still observed by other scans are retained — their last_seen
/// pointers are recomputed from the remaining observations.
pub fn delete_scan_cascade(scan_id: &str) -> Result<DeleteScanImpact, FindingsError> {
    let mut conn = open()?;
    let tx = conn.transaction()?;

    // Confirm the scan exists before we mutate anything.
    let _scan: ScanRecord = tx
        .query_row(
            "SELECT scan_id, aws_account_id, status, started_at, finished_at,
                    failure_code, warning_code, warning_detail, raw_output_path,
                    role_session_name, truncated
               FROM scans
              WHERE scan_id = ?1",
            params![scan_id],
            row_to_scan_record,
        )
        .optional()?
        .ok_or(FindingsError::ScanNotFound)?;

    // 1. Collect the finding_ids observed by this scan.
    let observed_findings: Vec<String> = {
        let mut stmt = tx.prepare("SELECT finding_id FROM scan_findings WHERE scan_id = ?1")?;
        let rows = stmt.query_map(params![scan_id], |row| row.get::<_, String>(0))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        out
    };

    // 2. Drop the join rows for this scan.
    tx.execute(
        "DELETE FROM scan_findings WHERE scan_id = ?1",
        params![scan_id],
    )?;

    let mut findings_removed = 0usize;
    let mut findings_updated = 0usize;
    let mut resources_removed = 0usize;

    for finding_id in &observed_findings {
        // Any remaining observations?
        let remaining: Option<(String, String)> = tx
            .query_row(
                "SELECT observed_at, scan_id FROM scan_findings
                  WHERE finding_id = ?1
               ORDER BY observed_at DESC
                  LIMIT 1",
                params![finding_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;

        match remaining {
            None => {
                // No other scan observed this finding — drop it AND its
                // resources. The resources are bound to the finding by
                // `finding_id`, so cascading by that key keeps the rule
                // "no orphan rows" exact.
                let resource_count: i64 = tx.query_row(
                    "SELECT COUNT(*) FROM finding_resources WHERE finding_id = ?1",
                    params![finding_id],
                    |row| row.get(0),
                )?;
                tx.execute(
                    "DELETE FROM finding_resources WHERE finding_id = ?1",
                    params![finding_id],
                )?;
                tx.execute(
                    "DELETE FROM findings WHERE finding_id = ?1",
                    params![finding_id],
                )?;
                findings_removed += 1;
                resources_removed += resource_count as usize;
            }
            Some((latest_ts, latest_scan_id)) => {
                // Recompute the row's last_seen_scan_id / last_seen_at so
                // it points at a still-existing scan. If the deleted scan
                // happened to be the first observation too, also collapse
                // first_seen_scan_id to the earliest remaining one.
                let earliest: (String, String) = tx.query_row(
                    "SELECT observed_at, scan_id FROM scan_findings
                      WHERE finding_id = ?1
                   ORDER BY observed_at ASC
                      LIMIT 1",
                    params![finding_id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )?;
                // Touch any resources whose last_seen_scan_id was the
                // deleted scan — pull them back to the new latest scan
                // so no row references a now-vanished scan_id.
                tx.execute(
                    "UPDATE finding_resources
                        SET last_seen_scan_id = ?1, last_seen_at = ?2
                      WHERE finding_id = ?3 AND last_seen_scan_id = ?4",
                    params![latest_scan_id, latest_ts, finding_id, scan_id],
                )?;
                tx.execute(
                    "UPDATE finding_resources
                        SET first_seen_scan_id = ?1, first_seen_at = ?2
                      WHERE finding_id = ?3 AND first_seen_scan_id = ?4",
                    params![earliest.1, earliest.0, finding_id, scan_id],
                )?;
                tx.execute(
                    "UPDATE findings
                        SET last_seen_at = ?1,
                            last_seen_scan_id = ?2
                      WHERE finding_id = ?3 AND last_seen_scan_id = ?4",
                    params![latest_ts, latest_scan_id, finding_id, scan_id],
                )?;
                tx.execute(
                    "UPDATE findings
                        SET first_seen_at = ?1,
                            first_seen_scan_id = ?2
                      WHERE finding_id = ?3 AND first_seen_scan_id = ?4",
                    params![earliest.0, earliest.1, finding_id, scan_id],
                )?;
                tx.execute(
                    "UPDATE findings
                        SET resolved_at = NULL, resolved_in_scan_id = NULL
                      WHERE finding_id = ?1 AND resolved_in_scan_id = ?2",
                    params![finding_id, scan_id],
                )?;
                findings_updated += 1;
            }
        }
    }

    // 3. Finally drop the scan row itself.
    tx.execute("DELETE FROM scans WHERE scan_id = ?1", params![scan_id])?;

    tx.commit()?;
    Ok(DeleteScanImpact {
        scan_id: scan_id.to_string(),
        findings_removed,
        resources_removed,
        findings_updated,
    })
}

/// Row → ScanRecord. Same shape as `scanner::storage::row_to_record`,
/// duplicated here so the findings module doesn't have a `pub(crate)`
/// dependency on the scanner's private helper.
fn row_to_scan_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<ScanRecord> {
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

fn row_to_finding(row: &rusqlite::Row<'_>) -> rusqlite::Result<Finding> {
    let finding_id: String = row.get(0)?;
    let aws_account_id: String = row.get(1)?;
    let rule_key: String = row.get(2)?;
    let raw_type: String = row.get(3)?;
    let service: String = row.get(4)?;
    let severity_str: String = row.get(5)?;
    let description: String = row.get(6)?;
    let rationale: Option<String> = row.get(7)?;
    let dashboard_name: Option<String> = row.get(8)?;
    let resource_path_pattern: Option<String> = row.get(9)?;
    let checked_items: i64 = row.get(10)?;
    let flagged_items: i64 = row.get(11)?;
    let status_str: String = row.get(12)?;
    let first_seen_at: String = row.get(13)?;
    let last_seen_at: String = row.get(14)?;
    let first_seen_scan_id: String = row.get(15)?;
    let last_seen_scan_id: String = row.get(16)?;
    let resolved_at: Option<String> = row.get(17)?;
    let resolved_in_scan_id: Option<String> = row.get(18)?;

    let severity = Severity::from_storage(&severity_str).unwrap_or_else(|| {
        eprintln!(
            "findings: unknown stored severity '{}'; falling back to informational",
            severity_str
        );
        Severity::Informational
    });
    let status = FindingStatus::from_storage(&status_str).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "unknown finding status",
            )),
        )
    })?;

    Ok(Finding {
        finding_id,
        aws_account_id,
        rule_key,
        raw_type,
        service,
        severity,
        description,
        rationale,
        dashboard_name,
        resource_path_pattern,
        checked_items,
        flagged_items,
        status,
        first_seen_at: parse_required_ts(first_seen_at)?,
        last_seen_at: parse_required_ts(last_seen_at)?,
        first_seen_scan_id,
        last_seen_scan_id,
        resolved_at: parse_optional_ts(resolved_at)?,
        resolved_in_scan_id,
    })
}

fn row_to_resource(row: &rusqlite::Row<'_>) -> rusqlite::Result<FindingResource> {
    let finding_id: String = row.get(0)?;
    let aws_account_id: String = row.get(1)?;
    let resource_path: String = row.get(2)?;
    let invalid: i64 = row.get(3)?;
    let first_seen_at: String = row.get(4)?;
    let last_seen_at: String = row.get(5)?;
    Ok(FindingResource {
        finding_id,
        aws_account_id,
        resource_path,
        invalid: invalid != 0,
        first_seen_at: parse_required_ts(first_seen_at)?,
        last_seen_at: parse_required_ts(last_seen_at)?,
    })
}

fn parse_required_ts(raw: String) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&raw)
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
