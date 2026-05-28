// Panic button — Contract 11D.
//
// "Settings → Panic" wipes every CloudSaw trace on the machine. The
// data wipe is IMMEDIATE and SYNCHRONOUS; only the app/installer
// self-delete is deferred to next boot (Contract 11 §Constraints).
//
// What gets wiped, in this order:
//   1. The SQLite database AND all pre-migration backups.
//   2. All raw scan output (`<data-root>/scans/`).
//   3. All Terraform working dirs + state (`<data-root>/tf-work/`).
//   4. All redacted logs (`<data-root>/logs/`).
//   5. The event log table content (covered by deleting the .db, but we
//      also call `eventlog::storage::wipe_all()` ahead of the file removal
//      so a transient db-locked race still removes the rows).
//   6. The app data root itself (`<data-root>/`).
//   7. Every CloudSaw entry in the OS keychain.
//
// Then, on platforms that support it, a small self-delete helper is
// staged to run on next boot/login and remove the installed app files.
// The running process cannot remove its own executable on Windows.
//
// After the wipe returns, the IPC bridge shows a native OS dialog (Reboot
// now / Later). "Later" is honored — there is no forced reboot. There is
// no "Cancel the wipe" option because the data is already gone (Contract
// 11 §Constraints).

pub mod selfdelete;

use serde::Serialize;

use crate::db::paths::app_data_dir;
use crate::eventlog::{self, EventInput, EventKind};
use crate::keychain::{self, KeychainWipeResult};

/// Summary returned to the UI after a panic wipe. Counts are best-effort
/// — the wipe is judged on the "did the data-root subtree get removed"
/// signal, not perfect per-file accounting.
#[derive(Debug, Clone, Serialize)]
pub struct PanicWipeResult {
    pub data_root_removed: bool,
    pub db_files_removed: usize,
    pub scan_dirs_removed: usize,
    pub tf_workdirs_removed: usize,
    pub log_files_removed: usize,
    pub event_log_rows_wiped: i64,
    pub keychain: KeychainWipeResult,
    /// True when the platform self-delete helper was successfully staged.
    /// `Later` reboot does NOT prevent the helper from running on the
    /// next boot — staging is a "best effort to fully remove app files"
    /// gesture, separate from the data-wipe outcome.
    pub self_delete_staged: bool,
}

/// Run the panic wipe. Synchronous. Errors are intentionally collapsed
/// into the `PanicWipeResult` rather than aborting partway — once the
/// user has confirmed panic, we must remove as much as possible even if
/// one subtree fails (Contract 11 §Acceptance Criteria: "if the helper
/// cannot run, the data wipe still fully succeeds").
pub fn run_panic_wipe(confirmation: &str) -> Result<PanicWipeResult, crate::errors::AppError> {
    run_wipe(
        confirmation,
        "PANIC",
        EventKind::PanicWipe,
        "Panic wipe invoked.",
        true,
    )
}

/// PR #67 — Reset Application: wipes all user data but keeps the app
/// installed. Same wipe pipeline as `run_panic_wipe`, but the
/// self-delete helper is NOT staged — the user wants the app to
/// restart fresh, not uninstall. The caller is expected to call
/// `AppHandle::restart()` after this returns so the wizard surfaces
/// on the next launch.
pub fn run_application_reset(
    confirmation: &str,
) -> Result<PanicWipeResult, crate::errors::AppError> {
    run_wipe(
        confirmation,
        "RESET",
        EventKind::PanicWipe,
        "Application reset invoked.",
        false,
    )
}

fn run_wipe(
    confirmation: &str,
    expected: &str,
    event_kind: EventKind,
    event_summary: &str,
    stage_self_delete: bool,
) -> Result<PanicWipeResult, crate::errors::AppError> {
    // Typed confirmation gate — same shape as the hard-delete pipeline.
    if confirmation != expected {
        return Err(crate::errors::AppError::ConfirmationRejected);
    }

    // Record the panic / reset in the event log BEFORE we wipe. The
    // row is about to be deleted, but the export-still-includes-it
    // path is covered by the "user exported before panic" workflow
    // described in the QA contract. Records as a stable count + path.
    let pre_event_count = eventlog::count_events().unwrap_or(-1);
    eventlog::record_event(
        EventInput::new(event_kind, event_summary).with_item_count(pre_event_count.max(0)),
    );

    // Best-effort: drain in-flight scanner children. We don't have a
    // global "scan cancel all" today; the registry-backed cancellation
    // path runs per scan ID. If the scanner module ever adds a sweep
    // we'll call it here. For now we just delete the scan-output dir,
    // which causes any in-flight write to error and the child to wind
    // down on its own.

    // 1+5. Drop event-log rows up front so a transient file-lock race
    //      can't leave a populated log next to a deleted db file.
    let event_log_rows_wiped = eventlog::count_events().unwrap_or(0);
    let _ = crate::eventlog::storage::wipe_all();

    let root = match app_data_dir() {
        Ok(p) => p,
        Err(e) => {
            return Err(crate::errors::AppError::Path(format!(
                "panic: could not resolve data dir: {e}"
            )))
        }
    };

    // 1. SQLite database + every pre-migration backup.
    let db_dir = root.join("db");
    let db_files_removed = remove_dir_count_files(&db_dir);

    // 2. Raw scan output.
    let scan_dirs_removed = remove_subdirs_count(&root.join("scans"));

    // 3. Terraform working dirs + state.
    let tf_workdirs_removed = remove_subdirs_count(&root.join("tf-work"));

    // 4. Logs.
    let log_files_removed = remove_dir_count_files(&root.join("logs"));

    // 6. The data root itself. After step 1-5 it should be empty; remove
    //    it for completeness. We try the recursive remove regardless so
    //    user data files added by future contracts (e.g. exports) also
    //    go away.
    let data_root_removed = std::fs::remove_dir_all(&root).is_ok();

    // 7. Keychain.
    let keychain = keychain::wipe_all();

    // Stage the self-delete helper for the panic path only. The
    // application-reset path keeps the app installed so the next
    // launch can surface the onboarding wizard — staging the helper
    // would defeat the purpose.
    let self_delete_staged = if stage_self_delete {
        selfdelete::stage_self_delete().is_ok()
    } else {
        false
    };

    Ok(PanicWipeResult {
        data_root_removed,
        db_files_removed,
        scan_dirs_removed,
        tf_workdirs_removed,
        log_files_removed,
        event_log_rows_wiped,
        keychain,
        self_delete_staged,
    })
}

/// Count files inside `dir` (one level deep) and remove the directory
/// recursively. Returns the file count we observed before the unlink.
fn remove_dir_count_files(dir: &std::path::Path) -> usize {
    let mut count = 0usize;
    if let Ok(reader) = std::fs::read_dir(dir) {
        for entry in reader.flatten() {
            if entry.path().is_file() {
                count += 1;
            }
        }
    }
    let _ = std::fs::remove_dir_all(dir);
    count
}

/// Count subdirectories (one level deep) under `parent` and recursively
/// remove `parent`. Returns the subdir count.
fn remove_subdirs_count(parent: &std::path::Path) -> usize {
    let mut count = 0usize;
    if let Ok(reader) = std::fs::read_dir(parent) {
        for entry in reader.flatten() {
            if entry.path().is_dir() {
                count += 1;
            }
        }
    }
    let _ = std::fs::remove_dir_all(parent);
    count
}
