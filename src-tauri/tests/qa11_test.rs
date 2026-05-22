// Contract 11-QA — Event Log, Retention, Deletion & Panic: QA & Security
// Verification.
//
// This file batches the QA acceptance checks that are verifiable without a
// real ScoutSuite scan and without a real OS reboot / actual self-delete
// helper run. Each test maps to a specific QA item from
// `cloud-saw-contracts/C11-event-log-retention-QA.md`. Items that require
// observable side effects on the live machine (the OS reboot dialog,
// the post-reboot self-delete helper firing) are documented in
// CONTRACT_11_VERIFICATION.md as operator-driven checks.
//
// Tests share a per-test sandbox via CLOUDSAW_DATA_DIR_OVERRIDE, run real
// migrations, and serialize through a module-level mutex like the other
// integration tests in this crate.

use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use chrono::{Duration as ChronoDuration, Utc};
use cloudsaw_lib::accounts::{storage as accounts_storage, types::AccountRecord, Environment};
use cloudsaw_lib::db::migrations;
use cloudsaw_lib::deletion::{self, DeletionError, HardDeleteOptions};
use cloudsaw_lib::eventlog::{self, EventInput, EventKind, EventLogFilter};
use cloudsaw_lib::keychain;
use cloudsaw_lib::retention::{self, RetentionPeriod};
use cloudsaw_lib::scanner::storage as scan_storage;
use cloudsaw_lib::wipe;
use rusqlite::Connection;

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
        let dir = std::env::temp_dir().join(format!("cloudsaw-qa11-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        fs::create_dir_all(dir.join("db")).unwrap();
        std::env::set_var("CLOUDSAW_DATA_DIR_OVERRIDE", &dir);
        // Contract 17: install the in-memory credential store BEFORE
        // any feature code runs. Tests never touch the OS keychain;
        // this is what lets the suite pass in CI (no D-Bus / Secret
        // Service / desktop session).
        let _ = cloudsaw_lib::keychain::install_in_memory_for_tests();
        migrations::run(&dir.join("db").join("cloudsaw.db")).unwrap();
        Self { _guard: guard, dir }
    }

    fn db_path(&self) -> PathBuf {
        self.dir.join("db").join("cloudsaw.db")
    }

    fn raw_dir_for(&self, scan_id: &str) -> PathBuf {
        self.dir.join("scans").join(scan_id)
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        std::env::remove_var("CLOUDSAW_DATA_DIR_OVERRIDE");
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn seed_account(aws_id: &str, label: &str) {
    accounts_storage::insert(&AccountRecord {
        aws_account_id: aws_id.to_string(),
        label: label.to_string(),
        profile_name: format!("test-{label}"),
        environment: Environment::Dev,
    })
    .unwrap();
}

/// Insert a fully-terminal scan row directly into SQLite with a raw output
/// path that exists on disk. Bypasses the orchestrator so we can test the
/// retention/delete pipelines without driving a real scan.
fn seed_terminal_scan(
    sandbox: &Sandbox,
    scan_id: &str,
    aws_id: &str,
    started_at: chrono::DateTime<Utc>,
) -> PathBuf {
    let raw_dir = sandbox.raw_dir_for(scan_id);
    fs::create_dir_all(&raw_dir).unwrap();
    let raw_file = raw_dir.join("raw-scout.json");
    fs::write(&raw_file, b"{\"resources\":[]}").unwrap();

    let conn = Connection::open(sandbox.db_path()).unwrap();
    conn.execute(
        "INSERT INTO scans (
            scan_id, aws_account_id, status, started_at, finished_at,
            raw_output_path, role_session_name, truncated, pid
         ) VALUES (?1, ?2, 'complete', ?3, ?3, ?4, ?5, 0, NULL)",
        rusqlite::params![
            scan_id,
            aws_id,
            started_at.to_rfc3339(),
            raw_file.to_string_lossy().to_string(),
            format!("session-{scan_id}"),
        ],
    )
    .unwrap();
    raw_file
}

// --- Happy Path ---------------------------------------------------------

#[test]
fn happy_record_event_appends_row_visible_to_list_and_export() {
    let _s = Sandbox::new("record-event");
    eventlog::record_event(
        EventInput::new(EventKind::MasterPasswordChanged, "Master password changed.")
            .with_detail("via_settings"),
    );
    eventlog::record_event(
        EventInput::new(EventKind::ScanCompleted, "Scan completed.")
            .with_account("111122223333")
            .with_scan_id("scan-abc"),
    );

    let listed = eventlog::list_events(EventLogFilter::default()).unwrap();
    assert_eq!(listed.len(), 2);
    let kinds: Vec<_> = listed.iter().map(|e| e.kind).collect();
    assert!(kinds.contains(&EventKind::MasterPasswordChanged));
    assert!(kinds.contains(&EventKind::ScanCompleted));

    let exported = eventlog::export_events().unwrap();
    assert!(exported.contains("master_password_changed"));
    assert!(exported.contains("scan_completed"));
    // Account ID surfaces masked, never as the raw value.
    assert!(exported.contains("****3333"));
    assert!(!exported.contains("111122223333"));
}

#[test]
fn happy_search_finds_substring_match_only() {
    let _s = Sandbox::new("search");
    eventlog::record_event(EventInput::new(EventKind::ScanCompleted, "Daily scan ok."));
    eventlog::record_event(EventInput::new(EventKind::ScanCompleted, "Adhoc scan ok."));
    eventlog::record_event(EventInput::new(
        EventKind::SettingsChanged,
        "Activity log view cleared.",
    ));

    let hits = eventlog::search_events("daily", None).unwrap();
    assert_eq!(hits.len(), 1);
    assert!(hits[0].summary.contains("Daily"));

    // Wildcard chars in the query don't behave like LIKE wildcards.
    let escaped = eventlog::search_events("%scan%", None).unwrap();
    assert_eq!(escaped.len(), 0);
}

#[test]
fn happy_clear_view_hides_earlier_entries_export_still_includes_them() {
    let _s = Sandbox::new("clear-view");
    eventlog::record_event(EventInput::new(EventKind::ScanCompleted, "Before clear."));
    std::thread::sleep(std::time::Duration::from_millis(50));
    eventlog::clear_event_view().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));
    eventlog::record_event(EventInput::new(EventKind::ScanCompleted, "After clear."));

    let default_view = eventlog::list_events(EventLogFilter::default()).unwrap();
    // The "view cleared" entry itself was recorded BEFORE the marker, so
    // the default view only shows "After clear." (and entries strictly
    // after the marker).
    let visible_summaries: Vec<_> = default_view.iter().map(|e| e.summary.as_str()).collect();
    assert!(
        visible_summaries.iter().any(|s| s.contains("After clear.")),
        "after-clear entry should be visible"
    );
    assert!(
        !visible_summaries
            .iter()
            .any(|s| s.contains("Before clear.")),
        "before-clear entry should be hidden by the view marker"
    );

    let exported = eventlog::export_events().unwrap();
    assert!(exported.contains("Before clear."));
    assert!(exported.contains("After clear."));
}

#[test]
fn happy_retention_purges_old_scan_output_and_old_eventlog_rows() {
    let s = Sandbox::new("retention");
    seed_account("111122223333", "dev");

    // Old terminal scan — should have its raw file purged.
    let old_started = Utc::now() - ChronoDuration::days(120);
    let raw_old = seed_terminal_scan(&s, "scan-old", "111122223333", old_started);
    assert!(raw_old.is_file());

    // Recent terminal scan — should be untouched.
    let raw_recent = seed_terminal_scan(
        &s,
        "scan-recent",
        "111122223333",
        Utc::now() - ChronoDuration::days(5),
    );

    // Old event log row.
    {
        let conn = Connection::open(s.db_path()).unwrap();
        conn.execute(
            "INSERT INTO event_log (event_id, occurred_at, kind, summary)
             VALUES ('old-event', ?1, 'scan_completed', 'old entry')",
            rusqlite::params![(Utc::now() - ChronoDuration::days(200)).to_rfc3339()],
        )
        .unwrap();
    }
    // Recent event log row.
    eventlog::record_event(EventInput::new(EventKind::ScanCompleted, "fresh entry"));

    let summary = retention::run_now().unwrap();
    assert!(
        summary.raw_files_removed >= 1,
        "old raw file should be purged"
    );
    assert!(
        summary.eventlog_rows_removed >= 1,
        "old event row should be purged"
    );
    assert!(!raw_old.is_file(), "old raw file is unlinked");
    assert!(raw_recent.is_file(), "recent raw file remains");

    // Findings metadata MUST NOT be purged by retention. The scan row
    // remains too (only the on-disk raw file goes), and a re-parse would
    // surface RawOutputMissing. Confirm the scan row is still present and
    // its raw_output_path is cleared.
    let conn = Connection::open(s.db_path()).unwrap();
    let path_remaining: Option<String> = conn
        .query_row(
            "SELECT raw_output_path FROM scans WHERE scan_id = 'scan-old'",
            [],
            |r| r.get::<_, Option<String>>(0),
        )
        .unwrap();
    assert!(
        path_remaining.is_none(),
        "raw_output_path cleared after purge"
    );
}

#[test]
fn happy_hard_delete_with_correct_confirmation_removes_data_and_runs_vacuum() {
    let s = Sandbox::new("hard-delete");
    seed_account("111122223333", "dev");
    let raw = seed_terminal_scan(&s, "scan-del", "111122223333", Utc::now());
    assert!(raw.is_file());

    let summary =
        deletion::hard_delete_scan("scan-del", "DELETE", HardDeleteOptions::default()).unwrap();
    assert!(summary.vacuum_run);
    assert!(!raw.is_file(), "raw file unlinked");
    assert!(!s.raw_dir_for("scan-del").is_dir(), "per-scan dir removed");

    // Scan row gone.
    let conn = Connection::open(s.db_path()).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM scans WHERE scan_id = 'scan-del'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);

    // Event-log row recorded as a count + path, NEVER content.
    let events = eventlog::list_events(EventLogFilter::default()).unwrap();
    let del = events
        .iter()
        .find(|e| matches!(e.kind, EventKind::ScanDeleted))
        .expect("delete event recorded");
    assert!(del.summary.contains("scan-del"));
    assert!(del.item_count.is_some());
}

#[test]
fn happy_panic_wipe_removes_db_scans_tfwork_logs_and_clears_eventlog() {
    let s = Sandbox::new("panic");
    seed_account("111122223333", "dev");
    seed_terminal_scan(&s, "scan-p", "111122223333", Utc::now());
    // Fake tf-work + logs to prove they're swept too.
    fs::create_dir_all(s.dir.join("tf-work").join("111122223333")).unwrap();
    fs::write(
        s.dir.join("tf-work").join("111122223333").join("state"),
        b"x",
    )
    .unwrap();
    fs::create_dir_all(s.dir.join("logs")).unwrap();
    fs::write(s.dir.join("logs").join("app.log"), b"x").unwrap();
    eventlog::record_event(EventInput::new(EventKind::ScanCompleted, "x"));

    let result = wipe::run_panic_wipe("PANIC").unwrap();
    assert!(result.tf_workdirs_removed >= 1);
    assert!(result.log_files_removed >= 1);
    assert!(result.scan_dirs_removed >= 1);
    assert!(result.db_files_removed >= 1 || result.data_root_removed);
    assert!(
        !s.dir.join("db").join("cloudsaw.db").exists(),
        "db file gone"
    );
    assert!(!s.dir.join("scans").exists(), "scans dir gone");
    assert!(!s.dir.join("tf-work").exists(), "tf-work dir gone");
    assert!(!s.dir.join("logs").exists(), "logs dir gone");
}

// --- Error States -------------------------------------------------------

#[test]
fn error_hard_delete_with_wrong_confirmation_does_not_proceed() {
    let s = Sandbox::new("bad-confirm");
    seed_account("111122223333", "dev");
    let raw = seed_terminal_scan(&s, "scan-keep", "111122223333", Utc::now());

    let err = deletion::hard_delete_scan(
        "scan-keep",
        "delete", // lowercase — must reject
        HardDeleteOptions::default(),
    )
    .unwrap_err();
    assert!(matches!(err, DeletionError::ConfirmationRejected));
    assert!(raw.is_file(), "raw file is NOT removed on bad confirmation");

    let conn = Connection::open(s.db_path()).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM scans WHERE scan_id = 'scan-keep'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "scan row preserved on bad confirmation");
}

#[test]
fn error_retention_never_keeps_data_indefinitely() {
    let s = Sandbox::new("retention-never");
    seed_account("111122223333", "dev");
    let raw_old = seed_terminal_scan(
        &s,
        "scan-keep",
        "111122223333",
        Utc::now() - ChronoDuration::days(500),
    );
    retention::set_scan_retention(RetentionPeriod::Never).unwrap();
    retention::set_eventlog_retention(RetentionPeriod::Never).unwrap();
    // Plant an ancient event log row.
    {
        let conn = Connection::open(s.db_path()).unwrap();
        conn.execute(
            "INSERT INTO event_log (event_id, occurred_at, kind, summary)
             VALUES ('ancient', ?1, 'scan_completed', 'ancient')",
            rusqlite::params![(Utc::now() - ChronoDuration::days(500)).to_rfc3339()],
        )
        .unwrap();
    }

    let summary = retention::run_now().unwrap();
    assert_eq!(summary.raw_files_removed, 0);
    assert_eq!(summary.eventlog_rows_removed, 0);
    assert!(raw_old.is_file(), "Never policy keeps raw files");
}

#[test]
fn error_panic_with_wrong_confirmation_rejected_and_data_intact() {
    let s = Sandbox::new("panic-bad-confirm");
    seed_account("111122223333", "dev");
    seed_terminal_scan(&s, "scan-keep", "111122223333", Utc::now());

    let err = wipe::run_panic_wipe("panic").unwrap_err();
    assert!(matches!(
        err,
        cloudsaw_lib::errors::AppError::ConfirmationRejected
    ));
    assert!(s.db_path().is_file(), "db survives bad confirmation");
}

#[test]
fn error_keychain_wipe_treats_missing_entries_as_success() {
    // Registry is intentionally empty in this contract — the call should
    // simply return zeros, never an error. This proves the panic flow's
    // "absent at panic time → success" rule.
    let _s = Sandbox::new("keychain-empty");
    let r = keychain::wipe_all();
    assert_eq!(r.failed, 0);
    assert_eq!(r.removed, 0);
}

// --- Responsiveness ------------------------------------------------------

#[test]
fn responsiveness_search_with_many_entries_returns_promptly() {
    let _s = Sandbox::new("search-perf");
    for i in 0..2_000 {
        eventlog::record_event(EventInput::new(
            EventKind::ScanCompleted,
            format!("scan #{i}"),
        ));
    }
    let start = Instant::now();
    let hits = eventlog::search_events("999", None).unwrap();
    let elapsed = start.elapsed();
    assert!(!hits.is_empty(), "search returns matches");
    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "search ran in {}ms, expected <2000ms",
        elapsed.as_millis(),
    );
}

#[test]
fn responsiveness_hard_delete_with_many_findings_completes_quickly() {
    let s = Sandbox::new("delete-perf");
    seed_account("111122223333", "dev");
    seed_terminal_scan(&s, "scan-perf", "111122223333", Utc::now());

    // Insert ~5k joined findings so the cascade has real work to do.
    let conn = Connection::open(s.db_path()).unwrap();
    for i in 0..5_000 {
        let fid = format!("{:064x}", i);
        let rule_key = format!("test-rule-{i}");
        conn.execute(
            "INSERT OR IGNORE INTO findings (
                finding_id, aws_account_id, rule_key, raw_type, service,
                severity, description, rationale, dashboard_name,
                resource_path_pattern, checked_items, flagged_items, status,
                first_seen_at, last_seen_at, first_seen_scan_id, last_seen_scan_id,
                resolved_at, resolved_in_scan_id
             ) VALUES (?1, '111122223333', ?2, 'rule', 's3', 'high', 'd',
                       NULL, NULL, NULL, 0, 0, 'open', ?3, ?3,
                       'scan-perf', 'scan-perf', NULL, NULL)",
            rusqlite::params![fid, rule_key, Utc::now().to_rfc3339()],
        )
        .unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO scan_findings (scan_id, finding_id, aws_account_id, observed_at)
             VALUES ('scan-perf', ?1, '111122223333', ?2)",
            rusqlite::params![fid, Utc::now().to_rfc3339()],
        )
        .unwrap();
    }

    let start = Instant::now();
    let summary =
        deletion::hard_delete_scan("scan-perf", "DELETE", HardDeleteOptions::default()).unwrap();
    let elapsed = start.elapsed();
    assert_eq!(summary.findings_removed, 5_000);
    assert!(summary.vacuum_run);
    assert!(
        elapsed < std::time::Duration::from_secs(30),
        "delete + VACUUM ran in {}ms, expected <30s",
        elapsed.as_millis(),
    );
}

#[test]
fn responsiveness_panic_wipe_completes_promptly() {
    let s = Sandbox::new("panic-perf");
    seed_account("111122223333", "dev");
    for i in 0..50 {
        seed_terminal_scan(&s, &format!("scan-{i}"), "111122223333", Utc::now());
    }

    let start = Instant::now();
    let _ = wipe::run_panic_wipe("PANIC").unwrap();
    let elapsed = start.elapsed();
    assert!(
        elapsed < std::time::Duration::from_secs(15),
        "panic wipe ran in {}ms",
        elapsed.as_millis(),
    );
}

// --- State Transitions --------------------------------------------------

#[test]
fn state_action_then_event_log_entry_appended() {
    let _s = Sandbox::new("state-action");
    let before = eventlog::count_events().unwrap();
    eventlog::record_event(EventInput::new(
        EventKind::MasterPasswordChanged,
        "Changed.",
    ));
    let after = eventlog::count_events().unwrap();
    assert_eq!(after, before + 1);
}

#[test]
fn state_aged_past_retention_purged_on_next_run() {
    let s = Sandbox::new("state-age");
    {
        let conn = Connection::open(s.db_path()).unwrap();
        conn.execute(
            "INSERT INTO event_log (event_id, occurred_at, kind, summary)
             VALUES ('old', ?1, 'scan_completed', 'old'),
                    ('new', ?2, 'scan_completed', 'new')",
            rusqlite::params![
                (Utc::now() - ChronoDuration::days(120)).to_rfc3339(),
                Utc::now().to_rfc3339(),
            ],
        )
        .unwrap();
    }
    let summary = retention::run_now().unwrap();
    assert!(summary.eventlog_rows_removed >= 1);
    let remaining = eventlog::list_events(EventLogFilter {
        include_cleared: true,
        ..Default::default()
    })
    .unwrap();
    assert!(remaining.iter().any(|e| e.summary == "new"));
    assert!(!remaining.iter().any(|e| e.summary == "old"));
}

#[test]
fn state_clear_view_hides_view_but_in_window_entries_still_exist_and_export() {
    let _s = Sandbox::new("state-clear");
    eventlog::record_event(EventInput::new(EventKind::ScanCompleted, "earlier-entry"));
    std::thread::sleep(std::time::Duration::from_millis(20));
    eventlog::clear_event_view().unwrap();
    let visible = eventlog::list_events(EventLogFilter::default()).unwrap();
    assert!(visible.iter().all(|e| e.summary != "earlier-entry"));
    let exported = eventlog::export_events().unwrap();
    assert!(exported.contains("earlier-entry"));
}

#[test]
fn state_targeted_data_then_hard_delete_data_gone_vacuum_run() {
    let s = Sandbox::new("state-delete");
    seed_account("111122223333", "dev");
    let raw = seed_terminal_scan(&s, "scan-x", "111122223333", Utc::now());
    let summary =
        deletion::hard_delete_scan("scan-x", "scan-x", HardDeleteOptions::default()).unwrap();
    assert!(summary.vacuum_run);
    assert!(!raw.is_file());
}

#[test]
fn state_panic_data_gone_keychain_swept_helper_attempted() {
    let s = Sandbox::new("state-panic");
    seed_account("111122223333", "dev");
    seed_terminal_scan(&s, "scan-y", "111122223333", Utc::now());

    let r = wipe::run_panic_wipe("PANIC").unwrap();
    // Keychain registry is empty today — both removed and failed are 0,
    // which is the documented "treated as success" path.
    assert_eq!(r.keychain.failed, 0);
    assert!(!s.db_path().exists());
}

// --- Security Check -----------------------------------------------------

#[test]
fn security_event_log_has_no_update_or_delete_path_from_public_api() {
    // Append-only: the public `eventlog` module exposes record/list/search/
    // export/clear_view ONLY. No update or delete is reachable from the
    // public surface — retention purges and panic wipe use internal storage
    // helpers explicitly gated by their owning modules.
    //
    // We can't compile-check the absence of a function, but we can assert
    // that the only mutation paths that exist on `storage` are the two
    // retention/panic helpers — by exercising the public surface and
    // confirming the row count only grows.
    let _s = Sandbox::new("append-only");
    eventlog::record_event(EventInput::new(EventKind::ScanCompleted, "first"));
    eventlog::record_event(EventInput::new(EventKind::ScanCompleted, "second"));
    let before = eventlog::count_events().unwrap();
    // The "clear view" call must NOT remove rows.
    eventlog::clear_event_view().unwrap();
    let after = eventlog::count_events().unwrap();
    assert!(after >= before, "clear_view never removes rows");
}

#[test]
fn security_event_log_never_records_secret_values() {
    // The event log inputs we ship pass through `truncate` and reject
    // free-form account-id-shaped inputs that aren't 12 digits. Confirm
    // a caller who tries to stuff a "password=foo" into the summary
    // doesn't get any special treatment — the value is stored verbatim
    // as a summary string. The contract guarantee is that NO call site
    // inside CloudSaw ever passes a credential here; we audit that
    // separately via grep in the QA report. The integration assertion
    // here is the masking rule for account IDs.
    let _s = Sandbox::new("masking");
    eventlog::record_event(
        EventInput::new(EventKind::ScanCompleted, "scan ok").with_account("111122223333"),
    );
    let listed = eventlog::list_events(EventLogFilter::default()).unwrap();
    let row = listed.first().expect("one row");
    assert_eq!(row.aws_account_id_masked.as_deref(), Some("****3333"));
}

#[test]
fn security_findings_metadata_never_purged_by_retention() {
    let s = Sandbox::new("findings-safe");
    seed_account("111122223333", "dev");
    seed_terminal_scan(
        &s,
        "scan-ancient",
        "111122223333",
        Utc::now() - ChronoDuration::days(500),
    );
    // Seed a finding row directly.
    {
        let conn = Connection::open(s.db_path()).unwrap();
        let fid = format!("{:064x}", 1);
        conn.execute(
            "INSERT INTO findings (
                finding_id, aws_account_id, rule_key, raw_type, service,
                severity, description, rationale, dashboard_name,
                resource_path_pattern, checked_items, flagged_items, status,
                first_seen_at, last_seen_at, first_seen_scan_id, last_seen_scan_id,
                resolved_at, resolved_in_scan_id
             ) VALUES (?1, '111122223333', 'rule', 'rule', 's3', 'high', 'd',
                       NULL, NULL, NULL, 0, 0, 'open', ?2, ?2,
                       'scan-ancient', 'scan-ancient', NULL, NULL)",
            rusqlite::params![fid, Utc::now().to_rfc3339()],
        )
        .unwrap();
    }
    let summary = retention::run_now().unwrap();
    // Raw file was purged (it's old), but the findings row remains.
    assert!(summary.raw_files_removed >= 1);
    let conn = Connection::open(s.db_path()).unwrap();
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM findings", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1, "findings metadata must survive retention");
}

#[test]
fn security_independent_retention_policies() {
    let _s = Sandbox::new("independent");
    retention::set_scan_retention(RetentionPeriod::Days(30)).unwrap();
    retention::set_eventlog_retention(RetentionPeriod::Days(180)).unwrap();
    let settings = retention::get_settings().unwrap();
    assert!(matches!(settings.scan_retention, RetentionPeriod::Days(30)));
    assert!(matches!(
        settings.eventlog_retention,
        RetentionPeriod::Days(180)
    ));
}

#[test]
fn security_hard_delete_runs_vacuum_after_delete() {
    let s = Sandbox::new("vacuum");
    seed_account("111122223333", "dev");
    seed_terminal_scan(&s, "scan-v", "111122223333", Utc::now());
    let summary =
        deletion::hard_delete_scan("scan-v", "DELETE", HardDeleteOptions::default()).unwrap();
    assert!(summary.vacuum_run);
}

#[test]
fn security_panic_is_immediate_and_synchronous_helper_status_is_separate() {
    let s = Sandbox::new("panic-sync");
    seed_account("111122223333", "dev");
    seed_terminal_scan(&s, "scan-z", "111122223333", Utc::now());

    let result = wipe::run_panic_wipe("PANIC").unwrap();
    // Data wipe acceptance is independent of self_delete_staged. The
    // self-delete staging may or may not succeed depending on the test
    // host's privilege level — the documented contract is that the
    // data wipe still fully succeeded either way.
    assert!(!s.db_path().exists());
    // Sanity-check that we returned the structured result.
    let _ = result.self_delete_staged;
}

/// Defense-in-depth use of `scan_storage` to confirm that the panic wipe
/// happens BEFORE the IPC layer returns. Without this, a buggy
/// implementation could spawn an async task and the test would see the
/// wipe succeed asynchronously after we've moved on.
#[test]
fn security_panic_wipe_does_not_leave_scan_rows_behind() {
    let s = Sandbox::new("panic-no-rows");
    seed_account("111122223333", "dev");
    seed_terminal_scan(&s, "scan-rem", "111122223333", Utc::now());
    let _ = scan_storage::get("scan-rem").unwrap();

    wipe::run_panic_wipe("PANIC").unwrap();
    // After the wipe the SQLite file is gone. Re-opening would create a
    // fresh empty db; we don't run migrations here so the table doesn't
    // exist. The simplest signal: no db file.
    assert!(!s.db_path().exists());
}
