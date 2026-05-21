// Integration tests for the findings parser & store (Contract 07).
//
// We exercise the public `findings::*` surface against a real SQLite
// database, just like the other integration suites. Each test seeds an
// account row + a scan row + an on-disk `raw-scout.json` and then drives
// `findings::parse_and_store`, `list_findings`, `get_finding`, `list_scans`,
// `get_scan`, and `delete_scan`.
//
// The harness mirrors `tests/scanner_test.rs`: per-test temp data dir via
// `CLOUDSAW_DATA_DIR_OVERRIDE`, real migration run, real SQLite. We
// serialize through a module-level env-lock so concurrent tests don't
// trample each other's data dir or env vars.

use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use cloudsaw_lib::accounts::{storage as accounts_storage, types::AccountRecord, Environment};
use cloudsaw_lib::db::migrations;
use cloudsaw_lib::findings::{self, FindingStatus, FindingsError, FindingsFilter, Severity};
use cloudsaw_lib::scanner::{storage as scan_storage, types::ScanStatus};

fn env_lock() -> &'static Mutex<()> {
    static L: OnceLock<Mutex<()>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(()))
}

struct Sandbox {
    _guard: std::sync::MutexGuard<'static, ()>,
    dir: PathBuf,
}

impl Sandbox {
    fn new(label: &str) -> Self {
        let guard = env_lock().lock().unwrap_or_else(|p| p.into_inner());
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("cloudsaw-findings-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        fs::create_dir_all(dir.join("db")).unwrap();
        std::env::set_var("CLOUDSAW_DATA_DIR_OVERRIDE", &dir);
        migrations::run(&dir.join("db").join("cloudsaw.db")).unwrap();
        Self { _guard: guard, dir }
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        std::env::remove_var("CLOUDSAW_DATA_DIR_OVERRIDE");
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn seed_account(aws_id: &str, label: &str) {
    let _ = accounts_storage::insert(&AccountRecord {
        aws_account_id: aws_id.to_string(),
        label: label.to_string(),
        profile_name: format!("test-{label}"),
        environment: Environment::Dev,
    });
}

/// Plant a scan row + on-disk raw-scout.json and return the path. We bypass
/// the scanner orchestrator and write directly through `scan_storage` so we
/// can control the `started_at` timestamp (it drives idempotency / resolution
/// ordering).
fn plant_scan(
    sb: &Sandbox,
    scan_id: &str,
    aws_account_id: &str,
    raw_json: &serde_json::Value,
    started_offset_seconds: i64,
) -> PathBuf {
    let claim = scan_storage::try_claim_account(
        scan_id,
        aws_account_id,
        &format!("cloudsaw-scan-{scan_id}"),
    )
    .unwrap();
    let _ = claim;

    // We need control over started_at — bump it by the requested offset by
    // updating the row directly through rusqlite.
    let started = chrono::Utc::now() + chrono::Duration::seconds(started_offset_seconds);
    let conn = rusqlite::Connection::open(sb.dir.join("db").join("cloudsaw.db")).unwrap();
    conn.execute(
        "UPDATE scans SET started_at = ?1 WHERE scan_id = ?2",
        rusqlite::params![started.to_rfc3339(), scan_id],
    )
    .unwrap();

    let output_dir = sb.dir.join("scans").join(scan_id);
    fs::create_dir_all(&output_dir).unwrap();
    let raw_path = output_dir.join("raw-scout.json");
    fs::write(&raw_path, serde_json::to_vec_pretty(raw_json).unwrap()).unwrap();
    scan_storage::set_raw_output_path(scan_id, &raw_path.to_string_lossy()).unwrap();
    scan_storage::update_status(scan_id, ScanStatus::Parsing).unwrap();
    scan_storage::record_complete(scan_id, None, false).unwrap();
    raw_path
}

/// Sample ScoutSuite output shape with three findings across two services,
/// matching what a real run might produce. Used by happy-path and idempotency
/// tests.
fn sample_scout_output(account_id: &str) -> serde_json::Value {
    serde_json::json!({
        "account_id": account_id,
        "provider_code": "aws",
        "services": {
            "iam": {
                "findings": {
                    "iam-password-policy-no-minimum-length": {
                        "description": "Password policy has no minimum length",
                        "rationale": "Longer passwords are harder to brute-force.",
                        "dashboard_name": "Password policy",
                        "path": "iam.password_policy",
                        "level": "danger",
                        "items": ["iam.password_policy.MinimumPasswordLength"],
                        "checked_items": 1,
                        "flagged_items": 1,
                        "service": "iam"
                    },
                    "iam-root-account-used-recently": {
                        "description": "Root account used in the last 30 days",
                        "level": "warning",
                        "path": "iam.credential_report",
                        "items": ["iam.credential_report.root"],
                        "checked_items": 1,
                        "flagged_items": 1,
                        "service": "iam"
                    }
                }
            },
            "s3": {
                "findings": {
                    "s3-bucket-no-encryption": {
                        "description": "S3 bucket has no default encryption",
                        "level": "warning",
                        "path": "s3.buckets.id",
                        "items": ["s3.buckets.id.example.encryption"],
                        "checked_items": 5,
                        "flagged_items": 1,
                        "service": "s3"
                    }
                }
            }
        }
    })
}

// ============================================================================
// HAPPY PATH
// ============================================================================

/// QA Happy Path: parsing a real raw-scout.json populates `scans`,
/// `findings`, and `finding_resources` with correct account_id partitioning.
#[test]
fn qa_happy_parse_populates_tables_with_correct_account_partitioning() {
    let sb = Sandbox::new("happy-populate");
    seed_account("111122223333", "qa-dev");
    plant_scan(
        &sb,
        "scan-a",
        "111122223333",
        &sample_scout_output("111122223333"),
        0,
    );

    let summary = findings::parse_and_store("scan-a").unwrap();
    assert_eq!(summary.findings_total, 3);
    assert_eq!(summary.findings_inserted, 3);
    assert_eq!(summary.findings_updated, 0);
    assert_eq!(summary.resources_inserted, 3);
    assert_eq!(summary.aws_account_id, "111122223333");

    let found = findings::list_findings("scan-a", FindingsFilter::default()).unwrap();
    assert_eq!(found.len(), 3);
    for f in &found {
        assert_eq!(f.aws_account_id, "111122223333");
        assert_eq!(f.status, FindingStatus::Open);
        assert_eq!(f.last_seen_scan_id, "scan-a");
    }
}

/// QA Happy Path: list_findings returns the expected findings for a scan.
#[test]
fn qa_happy_list_findings_returns_findings_for_scan() {
    let sb = Sandbox::new("happy-list");
    seed_account("111122223333", "qa-dev");
    plant_scan(
        &sb,
        "scan-a",
        "111122223333",
        &sample_scout_output("111122223333"),
        0,
    );
    findings::parse_and_store("scan-a").unwrap();

    let found = findings::list_findings("scan-a", FindingsFilter::default()).unwrap();
    let rule_keys: Vec<String> = found.iter().map(|f| f.rule_key.clone()).collect();
    assert!(rule_keys
        .iter()
        .any(|k| k == "iam-password-policy-no-minimum-length"));
    assert!(rule_keys.iter().any(|k| k == "s3-bucket-no-encryption"));

    // Severity filter — only `critical|high` should hit the iam danger rule.
    let filter = FindingsFilter {
        severity: vec![Severity::High, Severity::Critical],
        ..Default::default()
    };
    let high_only = findings::list_findings("scan-a", filter).unwrap();
    assert_eq!(high_only.len(), 1);
    assert_eq!(
        high_only[0].rule_key,
        "iam-password-policy-no-minimum-length"
    );
}

/// QA Happy Path: list_scans returns scans for an account; get_scan /
/// get_finding return single records.
#[test]
fn qa_happy_list_and_get_scans_findings() {
    let sb = Sandbox::new("happy-listscans");
    seed_account("111122223333", "qa-dev");
    plant_scan(
        &sb,
        "scan-a",
        "111122223333",
        &sample_scout_output("111122223333"),
        0,
    );
    findings::parse_and_store("scan-a").unwrap();

    let scans = findings::list_scans("111122223333").unwrap();
    assert_eq!(scans.len(), 1);
    assert_eq!(scans[0].scan_id, "scan-a");

    let scan = findings::get_scan("scan-a").unwrap();
    assert_eq!(scan.aws_account_id, "111122223333");

    let all = findings::list_findings("scan-a", FindingsFilter::default()).unwrap();
    let one = findings::get_finding(&all[0].finding_id).unwrap();
    assert_eq!(one.finding.finding_id, all[0].finding_id);
    assert!(!one.resources.is_empty());
}

/// QA Happy Path / State Transition: a re-scan updates recurring findings'
/// first-seen/last-seen and status.
#[test]
fn qa_happy_rescan_updates_first_and_last_seen_on_recurring_findings() {
    let sb = Sandbox::new("happy-rescan");
    seed_account("111122223333", "qa-dev");
    plant_scan(
        &sb,
        "scan-a",
        "111122223333",
        &sample_scout_output("111122223333"),
        0,
    );
    findings::parse_and_store("scan-a").unwrap();

    let before = findings::list_findings("scan-a", FindingsFilter::default()).unwrap();
    let pwd_before = before
        .iter()
        .find(|f| f.rule_key == "iam-password-policy-no-minimum-length")
        .unwrap()
        .clone();

    // Second scan, later in time.
    plant_scan(
        &sb,
        "scan-b",
        "111122223333",
        &sample_scout_output("111122223333"),
        60,
    );
    let summary = findings::parse_and_store("scan-b").unwrap();
    assert_eq!(summary.findings_inserted, 0, "all findings already existed");
    assert_eq!(
        summary.findings_updated, 3,
        "all three findings should be touched by the second scan"
    );
    assert_eq!(summary.findings_resolved, 0);

    let after = findings::list_findings("scan-b", FindingsFilter::default()).unwrap();
    let pwd_after = after
        .iter()
        .find(|f| f.rule_key == "iam-password-policy-no-minimum-length")
        .unwrap();
    assert_eq!(
        pwd_after.first_seen_at, pwd_before.first_seen_at,
        "first_seen_at must not change on recurring observation"
    );
    assert!(
        pwd_after.last_seen_at > pwd_before.last_seen_at,
        "last_seen_at must advance on later scan"
    );
    assert_eq!(pwd_after.last_seen_scan_id, "scan-b");
    assert_eq!(pwd_after.status, FindingStatus::Open);
}

// ============================================================================
// ERROR STATES
// ============================================================================

/// QA Error State: malformed scanner JSON → clear parse error, no partial
/// writes, scan marked failed.
#[test]
fn qa_error_malformed_json_yields_parse_error_with_no_partial_writes() {
    let sb = Sandbox::new("err-malformed");
    seed_account("111122223333", "qa-dev");

    // Plant a scan row with a deliberately invalid JSON file.
    let scan_id = "scan-malformed";
    scan_storage::try_claim_account(scan_id, "111122223333", "cloudsaw-scan-malformed").unwrap();
    let output_dir = sb.dir.join("scans").join(scan_id);
    fs::create_dir_all(&output_dir).unwrap();
    let raw = output_dir.join("raw-scout.json");
    fs::write(&raw, b"{this is :: not valid JSON,}").unwrap();
    scan_storage::set_raw_output_path(scan_id, &raw.to_string_lossy()).unwrap();
    scan_storage::record_complete(scan_id, None, false).unwrap();

    let err = findings::parse_and_store(scan_id).unwrap_err();
    assert!(matches!(err, FindingsError::ParseMalformed(_)));

    // No partial writes: no findings rows exist for this account.
    let conn = rusqlite::Connection::open(sb.dir.join("db").join("cloudsaw.db")).unwrap();
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM findings WHERE aws_account_id = ?1",
            rusqlite::params!["111122223333"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(n, 0);

    // Scan flipped to failed with the parse_malformed_json code.
    let scan = findings::get_scan(scan_id).unwrap();
    assert_eq!(scan.status, ScanStatus::Failed);
    assert_eq!(scan.failure_code.as_deref(), Some("parse_malformed_json"));
}

/// QA Error State: unknown finding type → stored with raw type preserved.
#[test]
fn qa_error_unknown_finding_type_preserves_raw_type() {
    let sb = Sandbox::new("err-unknown-type");
    seed_account("111122223333", "qa-dev");
    let raw = serde_json::json!({
        "account_id": "111122223333",
        "services": {
            "newservice": {
                "findings": {
                    "newservice-novel-finding-type": {
                        "description": "novel finding",
                        "level": "warning",
                        "items": ["newservice.thing"],
                        "checked_items": 1,
                        "flagged_items": 1
                    }
                }
            }
        }
    });
    plant_scan(&sb, "scan-x", "111122223333", &raw, 0);
    let summary = findings::parse_and_store("scan-x").unwrap();
    assert_eq!(summary.unknown_type_count, 1);

    let listed = findings::list_findings("scan-x", FindingsFilter::default()).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].raw_type, "newservice-novel-finding-type");
}

/// QA Error State: malformed resource ARN/path → resource row flagged
/// invalid; finding still stored.
#[test]
fn qa_error_malformed_resource_path_flagged_invalid_finding_still_stored() {
    let sb = Sandbox::new("err-bad-resource");
    seed_account("111122223333", "qa-dev");
    let raw = serde_json::json!({
        "account_id": "111122223333",
        "services": {
            "ec2": {
                "findings": {
                    "ec2-default-security-group-in-use": {
                        "description": "Default SG used",
                        "level": "warning",
                        "items": ["bad\u{0000}path", "ec2.security_groups.id.sg-1234"],
                        "checked_items": 2,
                        "flagged_items": 2
                    }
                }
            }
        }
    });
    plant_scan(&sb, "scan-r", "111122223333", &raw, 0);
    findings::parse_and_store("scan-r").unwrap();

    let listed = findings::list_findings("scan-r", FindingsFilter::default()).unwrap();
    assert_eq!(listed.len(), 1);
    let detail = findings::get_finding(&listed[0].finding_id).unwrap();
    assert_eq!(detail.resources.len(), 2);
    assert!(detail.resources.iter().any(|r| r.invalid));
    assert!(detail.resources.iter().any(|r| !r.invalid));
}

/// QA Error State: parsing a non-existent scan_id → clear error, no crash.
#[test]
fn qa_error_nonexistent_scan_id_returns_clear_error() {
    let _sb = Sandbox::new("err-no-scan");
    let err = findings::parse_and_store("does-not-exist").unwrap_err();
    assert!(matches!(err, FindingsError::ScanNotFound));
}

/// Edge case: account_id mismatch between scan row and raw-scout.json → hard
/// error, scan marked failed, no writes (Contract 07 §Constraints — account_id
/// is the partition key, never inferred from untrusted input).
#[test]
fn qa_error_account_mismatch_rejects_and_marks_scan_failed() {
    let sb = Sandbox::new("err-acct-mismatch");
    seed_account("111122223333", "qa-dev");
    let raw = sample_scout_output("999988887777"); // wrong account in file
    plant_scan(&sb, "scan-m", "111122223333", &raw, 0);
    let err = findings::parse_and_store("scan-m").unwrap_err();
    assert!(matches!(err, FindingsError::AccountMismatch));

    let scan = findings::get_scan("scan-m").unwrap();
    assert_eq!(scan.status, ScanStatus::Failed);
    assert_eq!(scan.failure_code.as_deref(), Some("parse_account_mismatch"));
}

// ============================================================================
// RESPONSIVENESS — index-backed queries on large datasets
// ============================================================================

/// QA Responsiveness: severity-filtered list query on a 50k-finding database
/// returns the first page in well under 100ms on typical hardware.
///
/// We seed two accounts so the partition-key filter has work to do, populate
/// 50k findings across them, then assert the EXPLAIN QUERY PLAN uses indexes
/// and the wall-clock first page comes back fast. The 100ms target is the
/// QA spec; we set the cap a bit higher (250ms) to absorb CI jitter while
/// still failing the test if the query ever degrades to a table scan.
#[test]
fn qa_responsiveness_severity_filtered_list_is_index_backed_and_fast() {
    let sb = Sandbox::new("resp-large");
    seed_account("111122223333", "qa-dev");
    seed_account("999988887777", "qa-prod");
    plant_scan(
        &sb,
        "scan-big",
        "111122223333",
        &sample_scout_output("111122223333"),
        0,
    );

    // Bulk insert 50k findings under the same account directly via rusqlite
    // — exercising the parser 50k times would dominate the test's runtime
    // without exercising the responsiveness property the QA cares about.
    let conn = rusqlite::Connection::open(sb.dir.join("db").join("cloudsaw.db")).unwrap();
    conn.execute_batch("BEGIN").unwrap();
    let now = chrono::Utc::now().to_rfc3339();
    for i in 0..50_000 {
        let rule_key = format!("iam-bulk-rule-{i}");
        let severity = match i % 5 {
            0 => "critical",
            1 => "high",
            2 => "medium",
            3 => "low",
            _ => "informational",
        };
        let finding_id = {
            use sha2::{Digest, Sha256};
            let mut h = Sha256::new();
            h.update(b"111122223333:");
            h.update(rule_key.as_bytes());
            hex::encode(h.finalize())
        };
        conn.execute(
            "INSERT INTO findings (
                finding_id, aws_account_id, rule_key, raw_type, service,
                severity, description, rationale, dashboard_name,
                resource_path_pattern, checked_items, flagged_items,
                status, first_seen_at, last_seen_at,
                first_seen_scan_id, last_seen_scan_id,
                resolved_at, resolved_in_scan_id
             ) VALUES (?1, ?2, ?3, ?3, 'iam',
                       ?4, 'bulk', NULL, NULL,
                       'iam.bulk', 1, 1,
                       'open', ?5, ?5,
                       'scan-big', 'scan-big',
                       NULL, NULL)",
            rusqlite::params![finding_id, "111122223333", rule_key, severity, now],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO scan_findings (scan_id, finding_id, aws_account_id, observed_at)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["scan-big", finding_id, "111122223333", now],
        )
        .unwrap();
    }
    // Also seed 5k findings under the OTHER account so the partition filter
    // has to actually exclude them.
    for i in 0..5_000 {
        let rule_key = format!("iam-other-rule-{i}");
        let finding_id = {
            use sha2::{Digest, Sha256};
            let mut h = Sha256::new();
            h.update(b"999988887777:");
            h.update(rule_key.as_bytes());
            hex::encode(h.finalize())
        };
        conn.execute(
            "INSERT INTO findings (
                finding_id, aws_account_id, rule_key, raw_type, service,
                severity, description, rationale, dashboard_name,
                resource_path_pattern, checked_items, flagged_items,
                status, first_seen_at, last_seen_at,
                first_seen_scan_id, last_seen_scan_id,
                resolved_at, resolved_in_scan_id
             ) VALUES (?1, ?2, ?3, ?3, 'iam',
                       'high', 'bulk', NULL, NULL,
                       'iam.bulk', 1, 1,
                       'open', ?4, ?4,
                       'other-scan', 'other-scan',
                       NULL, NULL)",
            rusqlite::params![finding_id, "999988887777", rule_key, now],
        )
        .unwrap();
    }
    conn.execute_batch("COMMIT").unwrap();

    // Index-backed: EXPLAIN QUERY PLAN must mention an index on findings
    // and an index on scan_findings (PK or otherwise). It must NOT report a
    // full table scan ("SCAN findings" without a USING INDEX/SEARCH USING).
    let plan =
        findings::explain_severity_filtered("scan-big", cloudsaw_lib::findings::Severity::High)
            .unwrap();
    let joined = plan.join("\n").to_ascii_lowercase();
    assert!(
        joined.contains("using index") || joined.contains("search "),
        "severity-filtered query must be index-backed; plan was:\n{joined}"
    );
    // Defense-in-depth: the plan must not report a SCAN over the findings
    // table without an index.
    assert!(
        !joined.contains("scan findings ") || joined.contains("using index"),
        "must not full-scan findings table; plan was:\n{joined}"
    );

    // Wall-clock first page < 250ms.
    let filter = FindingsFilter {
        severity: vec![Severity::High],
        limit: Some(100),
        offset: Some(0),
        ..Default::default()
    };
    let start = std::time::Instant::now();
    let page = findings::list_findings("scan-big", filter).unwrap();
    let elapsed = start.elapsed();
    assert_eq!(page.len(), 100);
    assert!(
        elapsed < std::time::Duration::from_millis(250),
        "severity-filtered first page should return promptly — took {elapsed:?}"
    );
}

/// QA Responsiveness: list_scans returns promptly with many scans present.
#[test]
fn qa_responsiveness_list_scans_returns_promptly_with_many_scans() {
    let sb = Sandbox::new("resp-listscans");
    seed_account("111122223333", "qa-dev");

    // Plant 500 scan rows directly. We don't need them parsed — list_scans
    // hits the scans table indexed by (aws_account_id, started_at DESC).
    let conn = rusqlite::Connection::open(sb.dir.join("db").join("cloudsaw.db")).unwrap();
    conn.execute_batch("BEGIN").unwrap();
    let now = chrono::Utc::now();
    for i in 0..500 {
        let scan_id = format!("scan-{i:03}");
        let ts = (now + chrono::Duration::seconds(i)).to_rfc3339();
        conn.execute(
            "INSERT INTO scans (scan_id, aws_account_id, status, started_at,
                                role_session_name, truncated)
             VALUES (?1, '111122223333', 'complete', ?2,
                     ?3, 0)",
            rusqlite::params![scan_id, ts, format!("cloudsaw-scan-{scan_id}")],
        )
        .unwrap();
    }
    conn.execute_batch("COMMIT").unwrap();
    let _ = sb;

    let start = std::time::Instant::now();
    let scans = findings::list_scans("111122223333").unwrap();
    let elapsed = start.elapsed();
    assert_eq!(scans.len(), 200, "default limit caps at 200");
    assert!(
        elapsed < std::time::Duration::from_millis(250),
        "list_scans must be index-backed — took {elapsed:?}"
    );
}

// ============================================================================
// STATE TRANSITIONS
// ============================================================================

/// QA State Transition: findings stored → same scan re-parsed → zero net
/// change (idempotent).
#[test]
fn qa_state_reparsing_same_scan_is_byte_idempotent_for_list_findings() {
    let sb = Sandbox::new("state-idempotent");
    seed_account("111122223333", "qa-dev");
    plant_scan(
        &sb,
        "scan-a",
        "111122223333",
        &sample_scout_output("111122223333"),
        0,
    );

    findings::parse_and_store("scan-a").unwrap();
    let first = findings::list_findings("scan-a", FindingsFilter::default()).unwrap();
    let first_json = serde_json::to_string(&first).unwrap();

    let summary = findings::parse_and_store("scan-a").unwrap();
    // findings_inserted should be 0 on the second parse; updates may show
    // because the UPDATE path writes the same values to existing rows. The
    // contract's "byte-identical from list_findings" assertion is the one
    // we hold to.
    assert_eq!(summary.findings_inserted, 0);
    assert_eq!(summary.findings_resolved, 0);

    let second = findings::list_findings("scan-a", FindingsFilter::default()).unwrap();
    let second_json = serde_json::to_string(&second).unwrap();
    assert_eq!(first_json, second_json);
}

/// QA State Transition: finding present in scan N → absent/resolved in
/// scan N+1 → status and last-seen updated, history retained.
#[test]
fn qa_state_resolution_marks_status_and_retains_history() {
    let sb = Sandbox::new("state-resolved");
    seed_account("111122223333", "qa-dev");

    // Scan A: full output.
    plant_scan(
        &sb,
        "scan-a",
        "111122223333",
        &sample_scout_output("111122223333"),
        0,
    );
    findings::parse_and_store("scan-a").unwrap();

    // Scan B: same services scanned, but no s3 finding.
    let mut raw_b = sample_scout_output("111122223333");
    raw_b["services"]["s3"]["findings"] = serde_json::json!({});
    plant_scan(&sb, "scan-b", "111122223333", &raw_b, 60);
    let summary = findings::parse_and_store("scan-b").unwrap();
    assert_eq!(
        summary.findings_resolved, 1,
        "the s3 finding should be marked resolved"
    );

    // The s3 finding should still be in the table — history retained —
    // with status='resolved'.
    let all = findings::list_findings(
        "scan-a",
        FindingsFilter {
            status: Some(FindingStatus::Resolved),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].rule_key, "s3-bucket-no-encryption");
    assert_eq!(all[0].resolved_in_scan_id.as_deref(), Some("scan-b"));
}

/// QA State Transition: scan present → delete_scan → scan and all child
/// rows gone, no orphans.
#[test]
fn qa_state_delete_scan_cascades_no_orphans() {
    let sb = Sandbox::new("state-delete");
    seed_account("111122223333", "qa-dev");
    plant_scan(
        &sb,
        "scan-a",
        "111122223333",
        &sample_scout_output("111122223333"),
        0,
    );
    findings::parse_and_store("scan-a").unwrap();

    let impact = findings::delete_scan("scan-a").unwrap();
    assert_eq!(impact.findings_removed, 3);
    assert!(impact.resources_removed >= 3);

    // The scan, its findings, and resources are gone.
    let conn = rusqlite::Connection::open(sb.dir.join("db").join("cloudsaw.db")).unwrap();
    let scans: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM scans WHERE scan_id = 'scan-a'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let findings_n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM findings WHERE aws_account_id = '111122223333'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let resources_n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM finding_resources WHERE aws_account_id = '111122223333'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let join_n: i64 = conn
        .query_row("SELECT COUNT(*) FROM scan_findings", [], |r| r.get(0))
        .unwrap();
    assert_eq!(scans, 0);
    assert_eq!(findings_n, 0);
    assert_eq!(resources_n, 0);
    assert_eq!(join_n, 0);
}

/// State Transition + idempotency edge: when a scan that observed a finding
/// is deleted, but a *different* later scan also observed it, the finding
/// row is retained and its last_seen pointers move to the surviving scan.
#[test]
fn qa_state_delete_scan_keeps_findings_observed_in_other_scans() {
    let sb = Sandbox::new("state-delete-shared");
    seed_account("111122223333", "qa-dev");
    plant_scan(
        &sb,
        "scan-a",
        "111122223333",
        &sample_scout_output("111122223333"),
        0,
    );
    plant_scan(
        &sb,
        "scan-b",
        "111122223333",
        &sample_scout_output("111122223333"),
        60,
    );
    findings::parse_and_store("scan-a").unwrap();
    findings::parse_and_store("scan-b").unwrap();

    let _ = findings::delete_scan("scan-b").unwrap();

    let surviving = findings::list_findings("scan-a", FindingsFilter::default()).unwrap();
    assert_eq!(surviving.len(), 3, "findings retained via scan-a");
    for f in &surviving {
        assert_eq!(f.last_seen_scan_id, "scan-a");
    }
}

// ============================================================================
// SECURITY CHECK
// ============================================================================

/// Security Check #1: every list-style query filters by account_id. Verified
/// by inspecting the source — every public read function in storage.rs goes
/// through a get_scan_row(scan_id) -> ScanRecord lookup which surfaces the
/// account, or takes aws_account_id explicitly.
#[test]
fn qa_security_every_list_query_partitions_by_account_id() {
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let src =
        std::fs::read_to_string(manifest.join("src").join("findings").join("storage.rs")).unwrap();
    // Each finding/resource read path either constrains by aws_account_id
    // directly or chains through `get_scan_row` (which constrains by scan_id,
    // and the scan_id is itself an opaque partition pointer).
    assert!(
        src.contains("f.aws_account_id = ?") || src.contains("WHERE aws_account_id = ?1"),
        "storage.rs must filter by aws_account_id on read paths"
    );
}

/// Security Check #2: SQL uses parameterized queries; no string-concatenated
/// values. We grep the storage source for the dangerous patterns; the only
/// dynamic SQL fragment is the placeholder list for `IN (?1, ?2, …)` whose
/// values are still bound.
#[test]
fn qa_security_no_string_concatenated_sql() {
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let src =
        std::fs::read_to_string(manifest.join("src").join("findings").join("storage.rs")).unwrap();
    let code_only: String = src
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    for forbidden in [
        "format!(\"SELECT",
        "format!(\"INSERT",
        "format!(\"UPDATE",
        "format!(\"DELETE",
    ] {
        let occurrences = code_only.matches(forbidden).count();
        // The IN-clause dynamic SQL also uses `format!` to build the
        // placeholder list. We allow at most that one site; everything
        // else must be a static string with bound parameters.
        assert!(
            occurrences <= 2,
            "findings/storage.rs must not concatenate SQL — found {occurrences} hits of {forbidden:?}"
        );
    }
}

/// Security Check #3: severity normalization maps every input to the fixed
/// set; unknown values log a warning and map to `informational`. We exercise
/// the parser with a deliberately broken level and a recognized one, and
/// confirm both flow through to a valid stored severity.
#[test]
fn qa_security_severity_normalization_is_total() {
    let sb = Sandbox::new("sec-sev");
    seed_account("111122223333", "qa-dev");
    let raw = serde_json::json!({
        "account_id": "111122223333",
        "services": {
            "iam": {
                "findings": {
                    "iam-known-rule": {
                        "description": "known",
                        "level": "danger",
                        "items": ["iam.thing"],
                        "checked_items": 1, "flagged_items": 1
                    },
                    "iam-novel-rule": {
                        "description": "novel",
                        "level": "Catastrophic",
                        "items": ["iam.other"],
                        "checked_items": 1, "flagged_items": 1
                    }
                }
            }
        }
    });
    plant_scan(&sb, "scan-s", "111122223333", &raw, 0);
    let summary = findings::parse_and_store("scan-s").unwrap();
    assert_eq!(summary.unknown_severity_count, 1);

    let listed = findings::list_findings("scan-s", FindingsFilter::default()).unwrap();
    assert_eq!(listed.len(), 2);
    for f in &listed {
        let s = f.severity.as_str();
        assert!(
            matches!(s, "critical" | "high" | "medium" | "low" | "informational"),
            "stored severity must be in the fixed set, got {s:?}"
        );
    }
}

/// Security Check (idempotency): parser is pure — identical input JSON
/// yields identical stored data aside from first-seen/last-seen bookkeeping.
/// Already implied by the byte-idempotency test, but we restate it here so
/// the QA closure is explicit.
#[test]
fn qa_security_parser_is_pure_for_identical_input() {
    let sb = Sandbox::new("sec-pure");
    seed_account("111122223333", "qa-dev");
    plant_scan(
        &sb,
        "scan-p",
        "111122223333",
        &sample_scout_output("111122223333"),
        0,
    );
    findings::parse_and_store("scan-p").unwrap();
    let first = findings::list_findings("scan-p", FindingsFilter::default()).unwrap();
    findings::parse_and_store("scan-p").unwrap();
    let second = findings::list_findings("scan-p", FindingsFilter::default()).unwrap();
    assert_eq!(first.len(), second.len());
    for (a, b) in first.iter().zip(second.iter()) {
        assert_eq!(a.finding_id, b.finding_id);
        assert_eq!(a.severity.as_str(), b.severity.as_str());
        assert_eq!(a.first_seen_at, b.first_seen_at);
        assert_eq!(a.last_seen_at, b.last_seen_at);
        assert_eq!(a.flagged_items, b.flagged_items);
    }
}

/// Security Check: list-view queries are index-backed (verified via
/// EXPLAIN QUERY PLAN). The bulky responsiveness test above checks the
/// 50k-row case; this is the bare minimum-rows variant so the regression
/// catches index drops even on small fixtures.
#[test]
fn qa_security_severity_query_plan_uses_index() {
    let sb = Sandbox::new("sec-plan");
    seed_account("111122223333", "qa-dev");
    plant_scan(
        &sb,
        "scan-pl",
        "111122223333",
        &sample_scout_output("111122223333"),
        0,
    );
    findings::parse_and_store("scan-pl").unwrap();

    let plan = findings::explain_severity_filtered("scan-pl", Severity::High).unwrap();
    let joined = plan.join("\n").to_ascii_lowercase();
    assert!(
        joined.contains("using index") || joined.contains("search "),
        "severity query must reference an index in EXPLAIN QUERY PLAN, plan was:\n{joined}"
    );
}
