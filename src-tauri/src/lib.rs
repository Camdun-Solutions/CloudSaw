// CloudSaw library crate. Hosts every privileged module and the Tauri runtime
// setup. The `run()` function is the single entry point invoked from `main.rs`
// (desktop) and from platform-specific entry points (mobile, future).

pub mod accounts;
pub mod ai;
pub mod applock;
pub mod auth;
pub mod db;
pub mod deletion;
pub mod errors;
pub mod eventlog;
pub mod findings;
pub mod github;
pub mod ipc;
pub mod keychain;
pub mod knowledgebase;
pub mod onboarding;
pub mod reports;
pub mod retention;
pub mod scanner;
pub mod scanner_role;
pub mod scheduler;
pub mod wipe;

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
        // Native save / open file dialog plugin used by the Contract 15
        // report exporter to source output paths.
        .plugin(tauri_plugin_dialog::init())
        // Tauri auto-updater (Contract 16). Configured in
        // `tauri.conf.json` with the production manifest URL and the
        // maintainer-provided Ed25519 public key. The plugin verifies
        // the signature of every fetched update before applying; an
        // unsigned or mis-signed update is rejected (Contract 16
        // §Constraints + §Security Check). The plugin is registered
        // unconditionally — the application UI is what decides whether
        // to PROMPT the user (notify-only); applying happens only on
        // explicit IPC.
        .plugin(tauri_plugin_updater::Builder::new().build())
        // PR #54 — desktop notification on scan completion. Frontend
        // wraps `@tauri-apps/plugin-notification` and only fires when
        // the user has opted in via the Settings → Notifications
        // toggle; the plugin itself just exposes the OS-native API.
        .plugin(tauri_plugin_notification::init())
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
            ipc::auth_create_profile,
            ipc::accounts_list,
            ipc::accounts_get,
            ipc::accounts_add,
            ipc::accounts_update,
            ipc::accounts_remove,
            ipc::accounts_get_active,
            ipc::accounts_set_active,
            ipc::accounts_get_display_settings,
            ipc::accounts_set_display_settings,
            ipc::scanner_role_requirements,
            ipc::scanner_role_connect,
            ipc::scanner_role_status,
            ipc::scanner_detect,
            ipc::scanner_run_scan,
            ipc::scanner_scan_status,
            ipc::scanner_cancel_scan,
            ipc::scanner_reveal_scan_dir,
            ipc::scanner_list_recent,
            ipc::findings_parse_and_store,
            ipc::findings_list,
            ipc::findings_get,
            ipc::findings_list_scans,
            ipc::findings_get_scan,
            ipc::findings_delete_scan,
            ipc::kb_get_article,
            ipc::kb_list_articles,
            ipc::kb_get_control_mappings,
            ipc::kb_list_frameworks,
            ipc::kb_get_refresh_settings,
            ipc::kb_set_refresh_settings,
            ipc::kb_check_for_update,
            ipc::kb_apply_update,
            ipc::scheduler_set_schedule,
            ipc::scheduler_get_schedule,
            ipc::scheduler_clear_schedule,
            ipc::scheduler_list_schedules,
            ipc::scheduler_next_run_times,
            ipc::scheduler_recent_events,
            ipc::eventlog_list,
            ipc::eventlog_search,
            ipc::eventlog_export,
            ipc::eventlog_clear_view,
            ipc::eventlog_count,
            ipc::retention_get_settings,
            ipc::retention_set_scan,
            ipc::retention_set_eventlog,
            ipc::retention_run_now,
            ipc::deletion_hard_delete_scan,
            ipc::deletion_vacuum_now,
            ipc::system_panic_wipe,
            ipc::system_request_reboot,
            ipc::github_get_settings,
            ipc::github_set_token,
            ipc::github_clear_token,
            ipc::github_set_findings_repo,
            ipc::github_generate_token_url,
            ipc::github_prepare_error_report,
            ipc::github_submit_error_report,
            ipc::github_browser_fallback_for_error,
            ipc::github_prepare_finding_ticket,
            ipc::github_submit_finding_ticket,
            ipc::github_browser_fallback_for_finding,
            ipc::github_get_finding_ticket,
            ipc::github_list_finding_tickets,
            ipc::ai_get_settings,
            ipc::ai_set_provider,
            ipc::ai_set_provider_key,
            ipc::ai_clear_provider_key,
            ipc::ai_has_provider_key,
            ipc::ai_set_business_context,
            ipc::ai_prepare_request,
            ipc::ai_send_request,
            ipc::onboarding_get_state,
            ipc::onboarding_set_language,
            ipc::onboarding_set_current_step,
            ipc::onboarding_mark_step_completed,
            ipc::onboarding_complete,
            ipc::onboarding_reset_for_rerun,
            ipc::report_export_scan_html,
            ipc::report_export_scan_pdf,
            ipc::report_export_custom_html,
            ipc::report_export_custom_pdf,
            ipc::report_preview_scan,
            ipc::report_preview_custom,
            ipc::report_get_settings,
            ipc::report_set_settings,
            // PR #64 — local demo data seeder. Gated by
            // `cfg(debug_assertions)` inside; release builds reject
            // the call and there is no UI surface for it in release.
            ipc::dev::dev_seed_demo_findings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn bootstrap() -> Result<std::sync::Arc<applock::SessionState>, AppError> {
    // Install the production OS-keychain-backed credential store
    // (Contract 17). Feature code (`github::pat`, `ai::key`,
    // `wipe::run_panic_wipe`) reads through the `keychain::store()`
    // accessor, which delegates to whatever was installed here.
    // Integration tests install an `InMemoryStore` instead — that's
    // what lets the qa11/qa12/qa13 suites run in CI runners with no
    // desktop session. CLAUDE.md §4.8.
    //
    // `install_store` returns Err iff a store is already installed.
    // That only happens on re-entry within the same process (e.g.
    // tests that import `lib::run`); the production runtime calls
    // `bootstrap` exactly once, so the result is always Ok here.
    // We swallow the error rather than panic.
    let _ = keychain::install_store(std::sync::Arc::new(keychain::KeyringStore::new()));

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

    // Load the knowledgebase content cache (bundled baseline, or the
    // remote cache if the user opted in and a valid cache is on disk).
    // Failures here are non-fatal — the UI's article surface still renders
    // a default-article placeholder, and a future refresh can recover.
    let _ = knowledgebase::bootstrap();

    // Round every persisted next_run_at forward to the first slot after
    // `now` so missed scheduled times (machine sleep, app closed) collapse
    // into a single catch-up per account. Failures here are non-fatal —
    // a stale anchor will be revisited on the next bootstrap.
    let _ = scheduler::runner::bootstrap_runner();
    // Spawn the background poll thread. Idempotent across re-entry.
    scheduler::runner::start_runner();

    // Retention sweep — Contract 11B. Best-effort; the engine purges raw
    // scan output and event-log rows older than their configured
    // retention windows. Findings metadata is intentionally never purged.
    retention::bootstrap_sweep();

    // App-started event. Records that the process came up so the
    // Activity Log can show "app started at X" without depending on
    // shell-level logging.
    eventlog::record_simple(eventlog::EventKind::AppStarted, "CloudSaw started.");

    // Decide whether the app starts locked or unlocked based on the stored
    // lock period and last_unlocked_at. Must happen AFTER migrations run.
    applock::bootstrap_session()
}
