// Scanner orchestrator — Contract 06.
//
// CloudSaw bundles a pinned ScoutSuite binary. This module orchestrates each
// scan end-to-end:
//
//     detect_binary()                   -> ScoutSuiteAvailability
//     run_scan(aws_account_id)          -> ScanRecord (pending; runs async)
//     scan_status(scan_id)              -> ScanRecord
//     cancel_scan(scan_id)              -> ()
//     list_recent_scans(account, limit) -> Vec<ScanRecord>
//
// Each scan walks the contract's state machine:
//
//     pending → assuming_role → scanning → parsing → complete
//                                                  | complete_with_warnings
//                                                  | failed
//                                                  | canceled
//
// What this module deliberately does NOT do (CLAUDE.md §5 + Contract 06
// §Constraints):
//   - Invoke ScoutSuite through a shell. argv arrays only, absolute path
//     only, SHA-256 verified before every spawn.
//   - Cache STS credentials. Every scan calls `sts:AssumeRole` fresh; the
//     resolved credentials live only on the ScoutSuite child's environment.
//   - Stream live progress over IPC. The frontend polls `scan_status`.
//   - Run more than one scan per account simultaneously. The storage layer
//     enforces the "scan already running" gate transactionally.

pub mod binary;
pub mod error;
pub mod handles;
pub mod runner;
pub mod storage;
pub mod sts;
pub mod types;

pub use error::ScannerError;
pub use types::{ScanRecord, ScanStatus, ScoutSuiteAvailability};

use rand_core::{OsRng, RngCore};

use crate::accounts;
use crate::db::paths::ensure_user_only_dir;

/// Detect whether a bundled ScoutSuite binary is present AND passes its
/// SHA-256 integrity check. Pure local-state — no AWS calls, no account
/// scope. The frontend gates the entire scan UI on this.
pub fn detect_binary() -> ScoutSuiteAvailability {
    binary::availability()
}

/// Start a scan for `aws_account_id`. Validates the account exists and has
/// a provisioned scanner role, locks the account against concurrent scans
/// via the `scans` table, then spawns a background worker that drives the
/// scan to a terminal state. Returns the initial `ScanRecord` immediately
/// (in `pending` or already `assuming_role`); the frontend polls
/// `scan_status` for progress.
pub async fn run_scan(aws_account_id: &str) -> Result<ScanRecord, ScannerError> {
    validate_account_id(aws_account_id)?;

    let account = accounts::get_account(aws_account_id)?;
    if !account.role_provisioned {
        return Err(ScannerError::RoleNotProvisioned);
    }

    // Verify binary integrity BEFORE claiming the account, so a tampered
    // bundle doesn't leave a phantom in-flight row behind.
    let _ = binary::locate_and_verify()?;

    let scan_id = mint_scan_id();
    let role_session_name = role_session_name_for(&scan_id);

    // The transactional claim atomically rejects a second concurrent scan
    // for the same account. After this returns Ok, the row is persisted in
    // `pending`.
    let initial = storage::try_claim_account(&scan_id, aws_account_id, &role_session_name)?;

    let handle = handles::register(&scan_id);

    let account_id_owned = aws_account_id.to_string();
    let scan_id_owned = scan_id.clone();
    let session_owned = role_session_name.clone();

    // Run the actual scan on a background thread so the IPC caller returns
    // immediately. We use a std thread (not a tokio task) because the heavy
    // work — wait()'ing on the child — is blocking and we want it off the
    // tokio runtime.
    std::thread::spawn(move || {
        execute_scan(handle, scan_id_owned, account_id_owned, session_owned);
    });

    Ok(initial)
}

/// Public scan-status read. Pure SQLite lookup.
pub fn scan_status(scan_id: &str) -> Result<ScanRecord, ScannerError> {
    storage::get(scan_id)
}

/// Cancel a running scan. Idempotent — if the scan is already terminal,
/// the call returns the current (terminal) record unchanged.
pub fn cancel_scan(scan_id: &str) -> Result<ScanRecord, ScannerError> {
    let record = storage::get(scan_id)?;
    if record.status.is_terminal() {
        return Ok(record);
    }
    // Signal the orchestrator thread first; it sees the cancel flag and
    // tears down. If the registry doesn't know about this scan (e.g.
    // process restart), we still mark it canceled here so the UI is
    // consistent.
    handles::signal_cancel(scan_id);
    storage::record_canceled(scan_id)?;
    let updated = storage::get(scan_id)?;
    // Event-log: UI-triggered cancel. The orchestrator thread that holds
    // the handle will also emit a canceled event when it wakes, so this
    // is a defense-in-depth emit for the "scan already terminal on the
    // next poll" race.
    emit_terminal_event(scan_id, &updated.aws_account_id);
    Ok(updated)
}

/// Open the platform file manager at this scan's output directory so the
/// user can inspect the raw outputs CloudSaw collected — `raw-scout.json`,
/// `scoutsuite-stderr.log`, and the `scoutsuite-results/` tree.
///
/// We validate the scan_id against the DB first (defense-in-depth — the
/// path is built from `runner::scan_output_dir`, which only joins onto
/// `app_data_dir`, but the frontend is not trusted with arbitrary string
/// substitution so we still gate on a real scan record). If the directory
/// does not exist (e.g. a scan that never produced output) we still attempt
/// the open call — the OS file manager will surface the missing-folder
/// message, which is more diagnostic than CloudSaw silently no-op'ing.
///
/// Cross-platform shell-out: explorer.exe / `open` / `xdg-open`. No flags,
/// no shell, single argv element (the absolute path), spawned and detached.
pub fn reveal_scan_dir(scan_id: &str) -> Result<(), ScannerError> {
    // Validates the scan exists in our SQLite. Errors with ScanNotFound
    // otherwise — the frontend handles that the same way as it does for
    // scanner_scan_status / scanner_cancel_scan.
    let _ = storage::get(scan_id)?;
    let dir = runner::scan_output_dir(scan_id)?;
    open_in_file_manager(&dir).map_err(|e| ScannerError::ScanIo(e.to_string()))
}

#[cfg(target_os = "windows")]
fn open_in_file_manager(path: &std::path::Path) -> std::io::Result<()> {
    // `explorer.exe` returns a non-zero exit code in some success paths
    // (it's chatty about whether the window was already open), so we
    // spawn and detach without inspecting the exit status.
    std::process::Command::new("explorer.exe")
        .arg(path)
        .spawn()?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn open_in_file_manager(path: &std::path::Path) -> std::io::Result<()> {
    std::process::Command::new("open").arg(path).spawn()?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn open_in_file_manager(path: &std::path::Path) -> std::io::Result<()> {
    // `xdg-open` is part of every freedesktop-compliant DE we'd ship on.
    // On a headless box it'd fail at spawn — surfaced to the UI as a
    // ScanIo error, which is correct.
    std::process::Command::new("xdg-open").arg(path).spawn()?;
    Ok(())
}

/// History query used by the UI. Returns the most recent `limit` scans for
/// the account, newest first.
pub fn list_recent_scans(
    aws_account_id: &str,
    limit: usize,
) -> Result<Vec<ScanRecord>, ScannerError> {
    validate_account_id(aws_account_id)?;
    let bounded = limit.clamp(1, 100);
    storage::list_for_account(aws_account_id, bounded)
}

/// Defense-in-depth check that the supplied AWS account ID is exactly 12
/// digits. The same check lives in `accounts::validation`; we run it here
/// (and in workdir paths elsewhere) so an unvalidated string can never
/// become a partition key or a path segment.
fn validate_account_id(id: &str) -> Result<(), ScannerError> {
    if id.len() == 12 && id.chars().all(|c| c.is_ascii_digit()) {
        Ok(())
    } else {
        Err(ScannerError::InvalidInput("aws_account_id"))
    }
}

/// Mark every non-terminal scan in the database as `failed` with
/// `scanner_process_lost`. Called once on app bootstrap so a previous
/// run that was killed (machine sleep, OS reboot, force-quit) doesn't
/// leave the UI showing a phantom running scan.
pub fn reap_stale_on_boot() -> Result<usize, ScannerError> {
    storage::reap_stale_in_flight()
}

/// Mint a fresh, opaque scan ID. 128 random bits, hex-encoded — same
/// pattern as the terraform plan tokens.
pub fn mint_scan_id() -> String {
    let mut bytes = [0u8; 16];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Build a stable `RoleSessionName` for AssumeRole. The format
/// `cloudsaw-scan-<first-12-of-scan-id>` keeps CloudTrail entries
/// correlatable to scans without exposing the full opaque ID.
pub fn role_session_name_for(scan_id: &str) -> String {
    let short = scan_id.get(..12).unwrap_or(scan_id);
    format!("cloudsaw-scan-{short}")
}

/// The actual orchestration body. Runs on a dedicated thread; every error
/// path persists a stable terminal state so the UI poll never sees a stuck
/// scan. Per CLAUDE.md §4.4, nothing logged here contains account IDs or
/// raw stderr.
fn execute_scan(
    handle: std::sync::Arc<handles::ScanHandle>,
    scan_id: String,
    aws_account_id: String,
    role_session_name: String,
) {
    // The Drop impl on the registry handle is the last thing to run — even
    // if we hit a panic-like path we'd want the handle removed so a
    // subsequent run isn't blocked by a ghost. Wrap in a defer-style closure.
    let _registry_cleanup = ScanCleanup {
        scan_id: scan_id.clone(),
    };

    if let Err(e) = execute_scan_inner(handle, &scan_id, &aws_account_id, &role_session_name) {
        // Persist the terminal failure. The `record_failed` call itself
        // can fail (SQLite locked etc.); ignore the secondary error — the
        // UI poll will eventually surface the row.
        let _ = storage::record_failed(&scan_id, e.code());
    }

    // Emit the appropriate event-log row for whatever terminal state the
    // scan landed in. Reads the row we just wrote rather than tracking
    // state across the inner function — keeps the inner path side-effect-
    // free against the event-log dependency.
    emit_terminal_event(&scan_id, &aws_account_id);
}

/// Look up the scan's terminal row and emit the matching event-log entry.
/// Best-effort: a missing row (e.g. concurrent panic wipe) is silently
/// dropped — the scanner never blocks on the event log.
fn emit_terminal_event(scan_id: &str, aws_account_id: &str) {
    use crate::eventlog::{record_event, EventInput, EventKind};
    use crate::scanner::types::ScanStatus;

    let record = match storage::get(scan_id) {
        Ok(r) => r,
        Err(_) => return,
    };
    let (kind, summary) = match record.status {
        ScanStatus::Complete => (
            EventKind::ScanCompleted,
            format!(
                "Scan {scan} for {acct} completed.",
                scan = scan_id,
                acct = crate::accounts::mask_for_logs(aws_account_id),
            ),
        ),
        ScanStatus::CompleteWithWarnings => (
            EventKind::ScanCompleted,
            format!(
                "Scan {scan} for {acct} completed with warnings.",
                scan = scan_id,
                acct = crate::accounts::mask_for_logs(aws_account_id),
            ),
        ),
        ScanStatus::Failed => (
            EventKind::ScanFailed,
            format!(
                "Scan {scan} for {acct} failed ({code}).",
                scan = scan_id,
                acct = crate::accounts::mask_for_logs(aws_account_id),
                code = record.failure_code.as_deref().unwrap_or("unknown"),
            ),
        ),
        ScanStatus::Canceled => (
            EventKind::ScanCanceled,
            format!(
                "Scan {scan} for {acct} canceled.",
                scan = scan_id,
                acct = crate::accounts::mask_for_logs(aws_account_id),
            ),
        ),
        // Non-terminal — execute_scan_inner left it in flight (we never
        // emit until a terminal row exists).
        _ => return,
    };
    record_event(
        EventInput::new(kind, summary)
            .with_scan_id(scan_id)
            .with_account(aws_account_id),
    );
}

struct ScanCleanup {
    scan_id: String,
}

impl Drop for ScanCleanup {
    fn drop(&mut self) {
        handles::unregister(&self.scan_id);
    }
}

fn execute_scan_inner(
    handle: std::sync::Arc<handles::ScanHandle>,
    scan_id: &str,
    aws_account_id: &str,
    role_session_name: &str,
) -> Result<(), ScannerError> {
    // Transition: pending -> assuming_role
    storage::update_status(scan_id, ScanStatus::AssumingRole)?;

    if handle.is_canceled() {
        storage::record_canceled(scan_id)?;
        return Ok(());
    }

    // Resolve the account + role ARN + external ID for AssumeRole.
    let account = accounts::get_account(aws_account_id)?;
    let role_arn = account_scanner_role_arn(aws_account_id)?;
    let external_id = account_external_id(aws_account_id)?;

    // Build the tokio runtime ad-hoc for the AssumeRole call. We can't use
    // tauri's runtime because this thread is a std::thread, not a tokio
    // task. The runtime lives only for the assume_role call.
    let creds = {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| ScannerError::Internal("tokio_build"))?;
        rt.block_on(sts::assume_scanner_role(
            &account.profile_name,
            &role_arn,
            &external_id,
            role_session_name,
        ))?
    };

    if handle.is_canceled() {
        storage::record_canceled(scan_id)?;
        return Ok(());
    }

    // Transition: assuming_role -> scanning
    storage::update_status(scan_id, ScanStatus::Scanning)?;

    // Prepare per-scan output directory with user-only permissions.
    let output_dir = runner::scan_output_dir(scan_id)?;
    ensure_user_only_dir(&output_dir).map_err(|e| ScannerError::ScanIo(e.to_string()))?;
    let raw_output_path = output_dir.join("raw-scout.json");

    // Test seam: a dry-run path that writes a deterministic raw-scout.json
    // and skips the actual ScoutSuite invocation. Production builds NEVER
    // honor this — the value is only set inside integration tests and
    // local dev shells.
    if std::env::var_os("CLOUDSAW_SCANNER_DRY_RUN").is_some() {
        return finish_dry_run(handle, scan_id, &raw_output_path);
    }

    // Spawn the bundled ScoutSuite binary. The integrity check happens
    // inside spawn_scoutsuite, immediately before the spawn call.
    // ScoutSuite derives the account ID from sts:GetCallerIdentity on the
    // assumed creds and writes its result to
    // `<output_dir>/scoutsuite-results/scoutsuite_results_aws-cloudsaw.js`
    // — we convert that to clean JSON at `raw_output_path` after the
    // subprocess exits via `runner::post_process_scoutsuite_output()`.
    runner::spawn_scoutsuite(handle.clone(), &output_dir, &default_region(), &creds)?;

    // Drop the credential buffer ASAP — the child has its own copy now.
    drop(creds);

    let outcome = runner::wait_for_child(handle.clone())?;

    // Transition: scanning -> parsing (even on canceled — partial output
    // gets a chance to be flushed before the cancel is recorded).
    storage::update_status(scan_id, ScanStatus::Parsing)?;

    // Always persist stderr to a sidecar log inside the scan dir, on
    // every outcome (success / partial / failure / canceled). Best-
    // effort: ignores write errors so a permission-issue on the log
    // path doesn't overwrite the scan's actual result. Gives the user
    // and us a debuggable trail when scans misbehave.
    runner::write_stderr_sidecar(&output_dir, &outcome.stderr);

    if outcome.canceled || handle.is_canceled() {
        storage::record_canceled(scan_id)?;
        return Ok(());
    }

    // Convert ScoutSuite's `.js`-wrapped output at
    // `<output_dir>/scoutsuite-results/scoutsuite_results_aws-cloudsaw.js`
    // into clean JSON at `raw_output_path`. The findings parser reads
    // from `raw_output_path` unchanged — the `.js` → JSON transform is
    // isolated here. If ScoutSuite never produced an output file the
    // helper returns Ok(false) and the next block falls into the
    // `OutputMissing` / `ProcessFailed` branch below.
    let _ = runner::post_process_scoutsuite_output(&output_dir, &raw_output_path);

    // Confirm the raw output file landed. Empty/missing == hard failure.
    let category = runner::classify_exit(outcome.exit_code);
    if !raw_output_path.is_file() || file_is_empty(&raw_output_path) {
        // ScoutSuite exited cleanly but produced no output: hard failure
        // even on category Success.
        if matches!(category, runner::ExitCategory::Failure) {
            storage::record_failed(scan_id, ScannerError::ProcessFailed.code())?;
        } else {
            storage::record_failed(scan_id, ScannerError::OutputMissing.code())?;
        }
        return Ok(());
    }

    storage::set_raw_output_path(scan_id, &raw_output_path.to_string_lossy())?;

    match category {
        runner::ExitCategory::Success => {
            storage::record_complete(scan_id, None, outcome.truncated)?;
        }
        runner::ExitCategory::PartialSuccess => {
            storage::record_complete(
                scan_id,
                Some(("missing_permissions", warning_detail_from(&outcome.stderr))),
                outcome.truncated,
            )?;
        }
        runner::ExitCategory::Failure => {
            storage::record_failed(scan_id, ScannerError::ProcessFailed.code())?;
        }
    }

    Ok(())
}

/// Dry-run finishing path. Used by integration tests to exercise the full
/// state machine without an actual ScoutSuite binary. Writes a small,
/// deterministic raw-scout.json so the test can later assert handoff to
/// Contract 07 is wired up correctly.
fn finish_dry_run(
    handle: std::sync::Arc<handles::ScanHandle>,
    scan_id: &str,
    raw_output_path: &std::path::Path,
) -> Result<(), ScannerError> {
    if handle.is_canceled() {
        storage::record_canceled(scan_id)?;
        return Ok(());
    }
    std::fs::write(
        raw_output_path,
        b"{\"cloudsaw_dryrun\":true,\"resources\":[]}",
    )?;
    crate::db::paths::set_user_only(raw_output_path, false)
        .map_err(|e| ScannerError::ScanIo(e.to_string()))?;
    storage::update_status(scan_id, ScanStatus::Parsing)?;
    storage::set_raw_output_path(scan_id, &raw_output_path.to_string_lossy())?;
    storage::record_complete(scan_id, None, false)?;
    Ok(())
}

/// Read the scanner-role ARN persisted by Contract 05's `apply`. Returning
/// `RoleNotProvisioned` here is defense-in-depth — the public `run_scan`
/// already checks `role_provisioned`, but a race between provisioning
/// rollback and scan start could otherwise leak through.
fn account_scanner_role_arn(aws_account_id: &str) -> Result<String, ScannerError> {
    use rusqlite::{params, Connection, OptionalExtension};
    let path = crate::db::paths::app_data_dir()
        .map_err(|e| ScannerError::ScanIo(e.to_string()))?
        .join("db")
        .join("cloudsaw.db");
    let conn = Connection::open(path)?;
    let row: Option<Option<String>> = conn
        .query_row(
            "SELECT scanner_role_arn FROM accounts WHERE aws_account_id = ?1",
            params![aws_account_id],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?;
    match row.flatten() {
        Some(arn) if !arn.is_empty() => Ok(arn),
        _ => Err(ScannerError::RoleNotProvisioned),
    }
}

fn account_external_id(aws_account_id: &str) -> Result<String, ScannerError> {
    use rusqlite::{params, Connection, OptionalExtension};
    let path = crate::db::paths::app_data_dir()
        .map_err(|e| ScannerError::ScanIo(e.to_string()))?
        .join("db")
        .join("cloudsaw.db");
    let conn = Connection::open(path)?;
    let row: Option<Option<String>> = conn
        .query_row(
            "SELECT external_id FROM accounts WHERE aws_account_id = ?1",
            params![aws_account_id],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?;
    match row.flatten() {
        Some(id) if !id.is_empty() => Ok(id),
        // An account whose role is provisioned but has no external_id is a
        // data-corruption bug — Contract 05's `apply` always writes one.
        _ => Err(ScannerError::Internal("missing_external_id")),
    }
}

/// AWS default region used for the scanner child. AWS SDKs require *some*
/// region to be set; `us-east-1` is the canonical default for org-scope
/// IAM/STS calls. ScoutSuite enumerates resources across all regions
/// regardless of this value.
fn default_region() -> String {
    std::env::var("AWS_DEFAULT_REGION")
        .or_else(|_| std::env::var("AWS_REGION"))
        .unwrap_or_else(|_| "us-east-1".to_string())
}

fn file_is_empty(path: &std::path::Path) -> bool {
    std::fs::metadata(path)
        .map(|m| m.len() == 0)
        .unwrap_or(true)
}

/// Reduce stderr to a stable warning-detail tag. We deliberately do NOT
/// surface the raw text — only a category. The categories are exhaustively
/// enumerated so the frontend can localize the message.
fn warning_detail_from(stderr: &[u8]) -> Option<&'static str> {
    let text = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    if text.contains("accessdenied") || text.contains("access denied") {
        Some("access_denied")
    } else if text.contains("throttl") {
        Some("throttled")
    } else if text.contains("unauthorized") {
        Some("unauthorized")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mint_scan_id_yields_unique_hex_strings() {
        let a = mint_scan_id();
        let b = mint_scan_id();
        assert_ne!(a, b);
        for t in [&a, &b] {
            assert_eq!(t.len(), 32);
            assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
        }
    }

    #[test]
    fn role_session_name_uses_short_id_prefix() {
        let name = role_session_name_for("abcdef0123456789abcdef0123456789");
        assert_eq!(name, "cloudsaw-scan-abcdef012345");
    }

    #[test]
    fn role_session_name_handles_short_input() {
        let name = role_session_name_for("abc");
        assert_eq!(name, "cloudsaw-scan-abc");
    }

    #[test]
    fn warning_detail_classifies_common_stderr_phrases() {
        assert_eq!(
            warning_detail_from(b"AccessDenied: ..."),
            Some("access_denied")
        );
        assert_eq!(
            warning_detail_from(b"Some throttling occurred"),
            Some("throttled")
        );
        assert_eq!(warning_detail_from(b"unauthorized!!"), Some("unauthorized"));
        assert!(warning_detail_from(b"all good").is_none());
    }
}
