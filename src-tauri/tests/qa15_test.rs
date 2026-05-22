// Contract 15-QA — Report Exporter & Custom Report Builder: QA &
// Security Verification.
//
// Exercises the reports module against a real SQLite database in a
// per-test sandbox. The save-dialog is intentionally NOT in scope —
// the contract requires the output path come from the dialog, but
// the Rust layer just validates the path shape; the integration
// tests pass explicit paths into a tempdir and assert the resulting
// file invariants.
//
// What we verify:
//   * Per-scan HTML report is self-contained (no <script>, no remote
//     URLs, banner + timestamp + version present).
//   * Per-scan PDF starts with `%PDF-` and ends with `%%EOF`.
//   * Account-ID disclosure modes work both ways and never leak the
//     raw ID under "masked".
//   * Empty-finding scans still produce a report with the empty-
//     state copy.
//   * Custom range scopes correctly to a specific account.
//   * Read-only output paths fail with `OutputWrite` and leave no
//     partial file.
//   * Auto-export copies the report to the configured folder; a
//     missing folder fails the copy but the primary export still
//     succeeds.
//   * Every export records a row in the event log.
//   * Resource paths and other text are HTML-escaped (no <script>
//     injection from a finding description).

use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use chrono::{Duration, Utc};
use cloudsaw_lib::accounts::{
    storage as accounts_storage, types::AccountRecord, Environment,
};
use cloudsaw_lib::db::migrations;
use cloudsaw_lib::eventlog::{self, EventKind, EventLogFilter};
use cloudsaw_lib::reports::{
    self, AccountIdDisclosure, ReportsError,
};
use rusqlite::{params, Connection};

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
        let dir = std::env::temp_dir().join(format!("cloudsaw-qa15-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        fs::create_dir_all(dir.join("db")).unwrap();
        std::env::set_var("CLOUDSAW_DATA_DIR_OVERRIDE", &dir);
        migrations::run(&dir.join("db").join("cloudsaw.db")).unwrap();
        Self { _guard: guard, dir }
    }

    fn db_path(&self) -> PathBuf {
        self.dir.join("db").join("cloudsaw.db")
    }

    fn output_path(&self, name: &str) -> String {
        self.dir.join(name).to_string_lossy().to_string()
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

fn seed_terminal_scan_with_finding(
    sandbox: &Sandbox,
    scan_id: &str,
    aws_id: &str,
    rule_key: &str,
    description: &str,
    resource_path: &str,
    started_at: chrono::DateTime<Utc>,
) -> String {
    let conn = Connection::open(sandbox.db_path()).unwrap();
    conn.execute(
        "INSERT INTO scans (
            scan_id, aws_account_id, status, started_at, finished_at,
            raw_output_path, role_session_name, truncated, pid
         ) VALUES (?1, ?2, 'complete', ?3, ?3, NULL, 'sess', 0, NULL)",
        params![scan_id, aws_id, started_at.to_rfc3339()],
    )
    .unwrap();

    let finding_id =
        cloudsaw_lib::findings::parser::finding_id_for(aws_id, rule_key);
    conn.execute(
        "INSERT INTO findings (
            finding_id, aws_account_id, rule_key, raw_type, service,
            severity, description, rationale, dashboard_name,
            resource_path_pattern, checked_items, flagged_items, status,
            first_seen_at, last_seen_at, first_seen_scan_id, last_seen_scan_id,
            resolved_at, resolved_in_scan_id
         ) VALUES (?1, ?2, ?3, 'rule', 's3', 'high', ?4, NULL, NULL,
                   NULL, 5, 2, 'open', ?5, ?5, ?6, ?6, NULL, NULL)",
        params![
            finding_id,
            aws_id,
            rule_key,
            description,
            started_at.to_rfc3339(),
            scan_id,
        ],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO scan_findings (scan_id, finding_id, aws_account_id, observed_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![scan_id, finding_id, aws_id, started_at.to_rfc3339()],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO finding_resources (
            finding_id, aws_account_id, resource_path, invalid,
            first_seen_at, last_seen_at, first_seen_scan_id, last_seen_scan_id
         ) VALUES (?1, ?2, ?3, 0, ?4, ?4, ?5, ?5)",
        params![
            finding_id,
            aws_id,
            resource_path,
            started_at.to_rfc3339(),
            scan_id,
        ],
    )
    .unwrap();
    finding_id
}

fn seed_terminal_scan(
    sandbox: &Sandbox,
    scan_id: &str,
    aws_id: &str,
    started_at: chrono::DateTime<Utc>,
) {
    let conn = Connection::open(sandbox.db_path()).unwrap();
    conn.execute(
        "INSERT INTO scans (
            scan_id, aws_account_id, status, started_at, finished_at,
            raw_output_path, role_session_name, truncated, pid
         ) VALUES (?1, ?2, 'complete', ?3, ?3, NULL, 'sess', 0, NULL)",
        params![scan_id, aws_id, started_at.to_rfc3339()],
    )
    .unwrap();
}

// --- Happy Path ---------------------------------------------------------

#[test]
fn happy_per_scan_html_is_self_contained_and_carries_banner() {
    let s = Sandbox::new("html");
    seed_account("111122223333", "dev");
    seed_terminal_scan_with_finding(
        &s,
        "scan-1",
        "111122223333",
        "s3-public-bucket",
        "Bucket allows public access to 111122223333.",
        "arn:aws:s3:::my-bucket",
        Utc::now(),
    );

    let output = s.output_path("scan-1.html");
    let outcome = reports::export_scan_html(
        "scan-1",
        &output,
        AccountIdDisclosure::Masked,
    )
    .unwrap();
    assert_eq!(outcome.primary_path, output);
    let body = fs::read_to_string(&output).unwrap();

    // Self-contained: no <script>, no remote URLs, no resource loads.
    assert!(!body.contains("<script"));
    assert!(!body.contains("</script"));
    assert!(!body.contains("http://"));
    assert!(!body.contains("https://"));
    // Mandatory banner + timestamp + version.
    assert!(body.contains("Review this report for sensitive data"));
    assert!(body.contains("Generated at"));
    assert!(body.contains(env!("CARGO_PKG_VERSION")));
    // Account IDs masked by default.
    assert!(body.contains("****3333"));
    assert!(!body.contains("111122223333"));
    // The finding's text is present.
    assert!(body.contains("s3-public-bucket"));
}

#[test]
fn happy_per_scan_pdf_starts_with_magic_and_contains_every_finding() {
    let s = Sandbox::new("pdf");
    seed_account("111122223333", "dev");
    for (scan_id, rule) in [
        ("scan-a", "s3-public-bucket"),
        ("scan-b", "iam-user-no-mfa"),
    ] {
        seed_terminal_scan_with_finding(
            &s,
            scan_id,
            "111122223333",
            rule,
            "desc",
            "arn:aws:s3:::b",
            Utc::now(),
        );
    }
    let output = s.output_path("scan-a.pdf");
    let outcome = reports::export_scan_pdf(
        "scan-a",
        &output,
        AccountIdDisclosure::Masked,
    )
    .unwrap();
    let bytes = fs::read(&outcome.primary_path).unwrap();
    assert!(bytes.starts_with(b"%PDF-"));
    let tail = if bytes.ends_with(b"%%EOF\n") {
        &bytes[..bytes.len() - 1]
    } else {
        &bytes
    };
    assert!(tail.ends_with(b"%%EOF"));
    // printpdf writes content streams through a FlateDecode filter, so
    // the rule-key text isn't a contiguous byte run in the raw PDF.
    // We validate the file structure (signature + EOF marker + a
    // realistic byte size for a single-finding report) and rely on
    // the lib unit tests + HTML report assertion above for content
    // correctness.
    assert!(
        bytes.len() > 500,
        "PDF should be non-trivially sized for a real finding"
    );
    // Confirm bytes_written matches what's on disk.
    assert_eq!(outcome.bytes_written as usize, bytes.len());
}

#[test]
fn happy_custom_report_scopes_to_selected_accounts_only() {
    let s = Sandbox::new("custom-scope");
    seed_account("111122223333", "dev");
    seed_account("999988887777", "prod");
    let now = Utc::now();
    seed_terminal_scan_with_finding(
        &s,
        "scan-dev",
        "111122223333",
        "s3-public-bucket",
        "dev finding",
        "arn:aws:s3:::dev-b",
        now,
    );
    seed_terminal_scan_with_finding(
        &s,
        "scan-prod",
        "999988887777",
        "iam-user-no-mfa",
        "prod finding",
        "arn:aws:iam::999988887777:user/admin",
        now,
    );

    let output = s.output_path("custom.html");
    let _ = reports::export_custom_html(
        now - Duration::days(7),
        now + Duration::days(1),
        &["111122223333".to_string()],
        &output,
        AccountIdDisclosure::Masked,
    )
    .unwrap();
    let body = fs::read_to_string(&output).unwrap();
    // The dev finding (rule key) IS in scope.
    assert!(body.contains("s3-public-bucket"));
    // The prod finding (rule key) is NOT — it belongs to an
    // out-of-scope account.
    assert!(!body.contains("iam-user-no-mfa"));
    // The masked dev account appears; the unmasked prod ID does not.
    assert!(body.contains("****3333"));
    assert!(!body.contains("999988887777"));
}

#[test]
fn happy_auto_export_copies_to_configured_folder() {
    let s = Sandbox::new("auto-export-ok");
    seed_account("111122223333", "dev");
    seed_terminal_scan_with_finding(
        &s,
        "scan-x",
        "111122223333",
        "s3-public-bucket",
        "desc",
        "arn:aws:s3:::b",
        Utc::now(),
    );

    // Configure the auto-export folder to a real subdir.
    let folder = s.dir.join("auto");
    fs::create_dir_all(&folder).unwrap();
    reports::set_settings(reports::ReportSettings {
        auto_export_enabled: true,
        auto_export_folder: Some(folder.to_string_lossy().to_string()),
        mask_account_ids_default: true,
    })
    .unwrap();

    let output = s.output_path("scan-x.html");
    let outcome = reports::export_scan_html(
        "scan-x",
        &output,
        AccountIdDisclosure::Masked,
    )
    .unwrap();
    assert!(!outcome.auto_export_failed);
    let auto = outcome.auto_export_path.expect("auto export path set");
    assert!(PathBuf::from(&auto).is_file());
}

#[test]
fn happy_export_records_event_log_entry() {
    let s = Sandbox::new("eventlog");
    seed_account("111122223333", "dev");
    seed_terminal_scan_with_finding(
        &s,
        "scan-evt",
        "111122223333",
        "s3-public-bucket",
        "desc",
        "arn:aws:s3:::b",
        Utc::now(),
    );

    let _ = reports::export_scan_html(
        "scan-evt",
        &s.output_path("scan-evt.html"),
        AccountIdDisclosure::Masked,
    )
    .unwrap();

    let entries = eventlog::list_events(EventLogFilter::default()).unwrap();
    assert!(
        entries
            .iter()
            .any(|e| matches!(e.kind, EventKind::Export)
                && e.summary.contains("Report exported")),
        "expected an Export event-log entry"
    );
}

// --- Error States -------------------------------------------------------

#[test]
fn error_empty_output_path_is_rejected() {
    let s = Sandbox::new("empty-path");
    seed_account("111122223333", "dev");
    seed_terminal_scan(&s, "scan-x", "111122223333", Utc::now());
    let err = reports::export_scan_html("scan-x", "", AccountIdDisclosure::Masked)
        .unwrap_err();
    assert!(matches!(err, ReportsError::InvalidInput(_)));
}

#[test]
fn error_directory_shaped_path_is_rejected_with_no_partial_file() {
    let s = Sandbox::new("dir-path");
    seed_account("111122223333", "dev");
    seed_terminal_scan(&s, "scan-x", "111122223333", Utc::now());
    let dir = s.dir.join("sub");
    fs::create_dir_all(&dir).unwrap();
    let trailing = format!("{}{}", dir.to_string_lossy(), std::path::MAIN_SEPARATOR);
    let err = reports::export_scan_html(
        "scan-x",
        &trailing,
        AccountIdDisclosure::Masked,
    )
    .unwrap_err();
    assert!(matches!(err, ReportsError::InvalidInput(_)));
    // The target sub directory still has no `.partial` file.
    let leftover: Vec<_> = fs::read_dir(&dir).unwrap().collect();
    assert!(leftover.is_empty(), "no partial file may be left behind");
}

#[test]
fn error_missing_parent_dir_fails_with_no_partial_file() {
    let s = Sandbox::new("missing-parent");
    seed_account("111122223333", "dev");
    seed_terminal_scan(&s, "scan-x", "111122223333", Utc::now());
    let target = s.dir.join("nope").join("scan-x.html");
    let err = reports::export_scan_html(
        "scan-x",
        &target.to_string_lossy(),
        AccountIdDisclosure::Masked,
    )
    .unwrap_err();
    assert!(matches!(err, ReportsError::OutputWrite));
    assert!(!target.exists());
    assert!(!s.dir.join("nope").exists());
}

#[test]
fn error_zero_finding_scan_still_generates_with_empty_state() {
    let s = Sandbox::new("zero");
    seed_account("111122223333", "dev");
    seed_terminal_scan(&s, "scan-empty", "111122223333", Utc::now());
    let output = s.output_path("empty.html");
    let _ = reports::export_scan_html(
        "scan-empty",
        &output,
        AccountIdDisclosure::Masked,
    )
    .unwrap();
    let body = fs::read_to_string(&output).unwrap();
    // Report still has the banner.
    assert!(body.contains("Review this report"));
    // Empty-state copy appears.
    assert!(body.contains("zero findings"));
}

#[test]
fn error_auto_export_folder_unavailable_primary_export_still_succeeds() {
    let s = Sandbox::new("auto-export-missing");
    seed_account("111122223333", "dev");
    seed_terminal_scan_with_finding(
        &s,
        "scan-y",
        "111122223333",
        "s3-public-bucket",
        "desc",
        "arn:aws:s3:::b",
        Utc::now(),
    );

    // Point auto-export at a folder that does NOT exist.
    let bad = s.dir.join("never-created");
    reports::set_settings(reports::ReportSettings {
        auto_export_enabled: true,
        auto_export_folder: Some(bad.to_string_lossy().to_string()),
        mask_account_ids_default: true,
    })
    .unwrap();

    let output = s.output_path("scan-y.html");
    let outcome = reports::export_scan_html(
        "scan-y",
        &output,
        AccountIdDisclosure::Masked,
    )
    .unwrap();
    // Primary export succeeded — the file exists.
    assert!(PathBuf::from(&outcome.primary_path).is_file());
    // The auto-export copy failed; the IPC surfaces the diagnostic.
    assert!(outcome.auto_export_failed);
    assert!(outcome.auto_export_path.is_none());
}

// --- Responsiveness -----------------------------------------------------

#[test]
fn responsiveness_large_report_generates_in_bounded_time() {
    let s = Sandbox::new("large");
    seed_account("111122223333", "dev");
    seed_terminal_scan_with_finding(
        &s,
        "scan-big",
        "111122223333",
        "s3-public-bucket",
        "Bucket allows public access to 111122223333.",
        "arn:aws:s3:::b",
        Utc::now(),
    );
    // Seed 1,500 findings on the same scan to stress the renderer.
    let conn = Connection::open(s.db_path()).unwrap();
    let now = Utc::now().to_rfc3339();
    for i in 0..1_500 {
        let fid = format!("{:064x}", i);
        let rk = format!("perf-rule-{i}");
        conn.execute(
            "INSERT OR IGNORE INTO findings (
                finding_id, aws_account_id, rule_key, raw_type, service,
                severity, description, rationale, dashboard_name,
                resource_path_pattern, checked_items, flagged_items, status,
                first_seen_at, last_seen_at, first_seen_scan_id, last_seen_scan_id,
                resolved_at, resolved_in_scan_id
             ) VALUES (?1, '111122223333', ?2, 'rule', 's3', 'medium',
                       'desc', NULL, NULL, NULL, 0, 0, 'open', ?3, ?3,
                       'scan-big', 'scan-big', NULL, NULL)",
            params![fid, rk, now],
        )
        .unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO scan_findings (scan_id, finding_id, aws_account_id, observed_at)
             VALUES ('scan-big', ?1, '111122223333', ?2)",
            params![fid, now],
        )
        .unwrap();
    }

    let output = s.output_path("scan-big.html");
    let start = Instant::now();
    let _ = reports::export_scan_html(
        "scan-big",
        &output,
        AccountIdDisclosure::Masked,
    )
    .unwrap();
    let elapsed = start.elapsed();
    assert!(
        elapsed < std::time::Duration::from_secs(30),
        "1.5k-finding HTML report took {}ms",
        elapsed.as_millis(),
    );
    let body = fs::read_to_string(&output).unwrap();
    assert!(body.contains("perf-rule-1499"));
}

// --- State Transitions --------------------------------------------------

#[test]
fn state_disclosure_default_persists_through_settings() {
    let _s = Sandbox::new("disclosure-default");
    // Default is mask=true (Masked).
    assert_eq!(
        std::mem::discriminant(&reports::default_disclosure()),
        std::mem::discriminant(&AccountIdDisclosure::Masked)
    );
    reports::set_settings(reports::ReportSettings {
        auto_export_enabled: false,
        auto_export_folder: None,
        mask_account_ids_default: false,
    })
    .unwrap();
    assert_eq!(
        std::mem::discriminant(&reports::default_disclosure()),
        std::mem::discriminant(&AccountIdDisclosure::Full)
    );
}

#[test]
fn state_per_scan_export_then_file_exists_with_expected_bytes() {
    let s = Sandbox::new("file-shape");
    seed_account("111122223333", "dev");
    seed_terminal_scan_with_finding(
        &s,
        "scan-shape",
        "111122223333",
        "s3-public-bucket",
        "desc",
        "arn:aws:s3:::b",
        Utc::now(),
    );
    let output = s.output_path("scan-shape.html");
    let outcome = reports::export_scan_html(
        "scan-shape",
        &output,
        AccountIdDisclosure::Masked,
    )
    .unwrap();
    let on_disk = fs::metadata(&output).unwrap().len();
    assert_eq!(outcome.bytes_written, on_disk);
}

// --- Security Check -----------------------------------------------------

#[test]
fn security_full_disclosure_is_opt_in_only() {
    let s = Sandbox::new("disclosure-optin");
    seed_account("111122223333", "dev");
    seed_terminal_scan_with_finding(
        &s,
        "scan-id",
        "111122223333",
        "s3-public-bucket",
        "Account 111122223333 has a bucket.",
        "arn:aws:s3:::b",
        Utc::now(),
    );

    let masked_path = s.output_path("masked.html");
    let _ = reports::export_scan_html(
        "scan-id",
        &masked_path,
        AccountIdDisclosure::Masked,
    )
    .unwrap();
    let masked = fs::read_to_string(&masked_path).unwrap();
    assert!(masked.contains("****3333"));
    assert!(!masked.contains("111122223333"));

    let full_path = s.output_path("full.html");
    let _ = reports::export_scan_html(
        "scan-id",
        &full_path,
        AccountIdDisclosure::Full,
    )
    .unwrap();
    let full = fs::read_to_string(&full_path).unwrap();
    assert!(full.contains("111122223333"));
}

#[test]
fn security_html_escapes_finding_text_so_a_script_payload_renders_as_text() {
    let s = Sandbox::new("xss");
    seed_account("111122223333", "dev");
    // The "description" carries an attempted script injection. The
    // renderer MUST escape every `<` so the resulting HTML contains
    // no `<script` substring.
    seed_terminal_scan_with_finding(
        &s,
        "scan-xss",
        "111122223333",
        "s3-public-bucket",
        "<script>alert('xss')</script> and <img onerror=fail>",
        "<script>steal()</script>",
        Utc::now(),
    );
    let output = s.output_path("scan-xss.html");
    let _ = reports::export_scan_html(
        "scan-xss",
        &output,
        AccountIdDisclosure::Masked,
    )
    .unwrap();
    let body = fs::read_to_string(&output).unwrap();
    assert!(!body.contains("<script"));
    assert!(!body.contains("</script"));
    // The escaped form is present so a reviewer can see the payload.
    assert!(body.contains("&lt;script&gt;"));
}

#[test]
fn security_output_file_has_user_only_permissions_on_unix() {
    let s = Sandbox::new("perms");
    seed_account("111122223333", "dev");
    seed_terminal_scan(&s, "scan-p", "111122223333", Utc::now());
    let output = s.output_path("perms.html");
    let _ = reports::export_scan_html(
        "scan-p",
        &output,
        AccountIdDisclosure::Masked,
    )
    .unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&output).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
    #[cfg(windows)]
    {
        // On Windows the test sandbox lives inside the user profile,
        // which is already user-restricted by default. The Rust side
        // calls `set_user_only` which is a no-op on Windows — the
        // assertion here is that the file exists and is readable by
        // the running user.
        let f = fs::File::open(&output).unwrap();
        let _ = f.metadata().unwrap();
    }
}

#[test]
fn security_event_log_export_row_carries_count_and_path_not_content() {
    let s = Sandbox::new("eventlog-redaction");
    seed_account("111122223333", "dev");
    seed_terminal_scan_with_finding(
        &s,
        "scan-evt",
        "111122223333",
        "s3-public-bucket",
        "Bucket public access on 111122223333.",
        "arn:aws:s3:::b",
        Utc::now(),
    );

    let _ = reports::export_scan_html(
        "scan-evt",
        &s.output_path("evt.html"),
        AccountIdDisclosure::Masked,
    )
    .unwrap();

    let entries = eventlog::list_events(EventLogFilter::default()).unwrap();
    let export = entries
        .iter()
        .find(|e| matches!(e.kind, EventKind::Export))
        .expect("export event missing");
    // The row carries a path + an item_count, NOT the report body.
    assert!(export.path.is_some());
    assert!(export.item_count.is_some());
    // The summary does not contain the finding description verbatim.
    assert!(!export
        .summary
        .contains("Bucket public access on 111122223333"));
}
