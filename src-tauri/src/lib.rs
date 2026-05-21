// CloudSaw library crate. Hosts every privileged module and the Tauri runtime
// setup. The `run()` function is the single entry point invoked from `main.rs`
// (desktop) and from platform-specific entry points (mobile, future).

pub mod accounts;
pub mod applock;
pub mod auth;
pub mod db;
pub mod errors;
pub mod findings;
pub mod ipc;
pub mod knowledgebase;
pub mod reports;
pub mod scanner;
pub mod terraform;

use crate::errors::AppError;

/// Application entry point. Initializes app data dirs, runs SQLite migrations,
/// registers IPC commands, and launches the Tauri runtime.
pub fn run() {
    let session = match bootstrap() {
        Ok(s) => s,
        Err(err) => {
            // Bootstrap failures happen before the UI exists, so we can't
            // surface a stable error code through IPC. Log to stderr and exit
            // non-zero so the OS / CI / user sees the problem rather than a
            // silent crash.
            eprintln!("cloudsaw bootstrap failed: {err}");
            std::process::exit(1);
        }
    };

    tauri::Builder::default()
        .manage(session)
        .invoke_handler(tauri::generate_handler![
            ipc::app_version,
            ipc::applock_get_state,
            ipc::applock_set_master_password,
            ipc::applock_unlock,
            ipc::applock_unlock_with_biometric,
            ipc::applock_lock,
            ipc::applock_change_password,
            ipc::applock_recovery_unlock,
            ipc::applock_get_settings,
            ipc::applock_set_settings,
            ipc::applock_verify_password,
            ipc::auth_list_profiles,
            ipc::auth_get_caller_identity,
            ipc::auth_test_profile,
            ipc::accounts_list,
            ipc::accounts_get,
            ipc::accounts_add,
            ipc::accounts_update,
            ipc::accounts_remove,
            ipc::accounts_get_active,
            ipc::accounts_set_active,
            ipc::accounts_get_display_settings,
            ipc::accounts_set_display_settings,
            ipc::terraform_detect,
            ipc::terraform_plan,
            ipc::terraform_apply,
            ipc::terraform_provisioning_status,
            ipc::scanner_detect,
            ipc::scanner_run_scan,
            ipc::scanner_scan_status,
            ipc::scanner_cancel_scan,
            ipc::scanner_list_recent,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn bootstrap() -> Result<std::sync::Arc<applock::SessionState>, AppError> {
    let data_root = db::paths::app_data_dir()?;
    db::paths::ensure_user_only_dir(&data_root)?;

    let db_dir = data_root.join("db");
    db::paths::ensure_user_only_dir(&db_dir)?;

    let db_path = db_dir.join("cloudsaw.db");
    db::migrations::run(&db_path)?;

    // Reap any in-flight scans left over from a prior process that was
    // killed mid-scan (machine sleep, force-quit, crash). Contract 06
    // §Edge Cases: stale scans must be visible as `failed` rather than
    // appearing to still be running. This runs after migrations so the
    // `scans` table exists; failures here are non-fatal — the scans UI
    // will eventually self-heal as new scans land.
    let _ = scanner::reap_stale_on_boot();

    // Decide whether the app starts locked or unlocked based on the stored
    // lock period and last_unlocked_at. Must happen AFTER migrations run.
    applock::bootstrap_session()
}
