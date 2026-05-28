// IPC surface. Every `#[tauri::command]` declared here MUST:
//   - validate its inputs (no command trusts a value from the frontend)
//   - return `Result<T, AppError>` (or an infallible primitive)
//   - never accept or return credential-bearing types
//
// IPC payloads use plain serializable structs. AWS SDK types never cross this
// boundary. See CLAUDE.md §4.1.

// PR #64 — `dev_seed_demo_findings`. Compiles unconditionally so the
// handler list in `lib.rs::run` can reference it; the body itself is
// gated by `cfg(debug_assertions)` and rejects the call on release
// builds.
pub mod dev;

use std::sync::Arc;

use tauri::State;
use zeroize::Zeroizing;

use crate::accounts::{
    self, Account, AccountsDisplaySettings, AddAccountInput, RemovalImpact, UpdateAccountInput,
};
use crate::ai::{self, AiRequestPreview, AiSettings, AiSuggestion, BusinessContext, Provider};
use crate::applock::{self, LockSettings, LockState, SessionState};
use crate::auth::{self, CallerIdentity, ProfileInfo, ProfileTestResult};
use crate::deletion::{self, HardDeleteOptions, HardDeleteSummary};
use crate::errors::AppError;
use crate::eventlog::{self, EventLogEntry, EventLogFilter};
use crate::findings::{
    self, DeleteScanImpact, Finding, FindingDetail, FindingsFilter, ParseSummary,
};
use crate::github::{
    self, BrowserSubmission, FindingTicket, GithubSettings, IssueCreated, IssuePreview,
    RepoSelection,
};
use crate::knowledgebase::{
    self, ArticleSummary, ControlMapping, Framework, KnowledgeArticle, RefreshApplyResult,
    RefreshCheckResult, RefreshSettings, RefreshSettingsUpdate,
};
use crate::onboarding::{self, OnboardingState, OnboardingStep};
use crate::reports::{self, AccountIdDisclosure, ExportOutcome, ReportContent, ReportSettings};
use crate::retention::{self, RetentionPeriod, RetentionRunSummary, RetentionSettings};
use crate::scanner::{self, ScanRecord, ScoutSuiteAvailability};
use crate::scanner_role::{
    self, ConnectResult, PolicyVariant, ProvisioningStatus, RoleRequirements,
};
use crate::scheduler::{self, NextRunTime, Schedule, ScheduleEvent, SetScheduleInput};
use crate::wipe::{self, PanicWipeResult};

/// Returns the running CalVer build string (e.g. "2026.5.0").
///
/// Trivially derived from `CARGO_PKG_VERSION` at compile time. Exposed so the
/// UI can render "About" / update-banner copy from one source of truth.
#[tauri::command]
pub fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

// --- App lock (Contract 02) ---------------------------------------------
//
// The frontend reads `applock_get_state` on mount and re-reads it after every
// state-changing call so it can decide which gate screen (first-run setup /
// unlock / main app) to render.

#[tauri::command]
pub fn applock_get_state(session: State<'_, Arc<SessionState>>) -> Result<LockState, AppError> {
    applock::get_state(session.inner())
}

#[tauri::command]
pub fn applock_set_master_password(
    session: State<'_, Arc<SessionState>>,
    password: String,
) -> Result<(), AppError> {
    applock::set_master_password(session.inner(), Zeroizing::new(password))
}

#[tauri::command]
pub fn applock_unlock(
    session: State<'_, Arc<SessionState>>,
    password: String,
) -> Result<(), AppError> {
    applock::unlock(session.inner(), Zeroizing::new(password))
}

/// Triggers the OS biometric prompt and unlocks on success. `reason` is the
/// message shown to the user inside the prompt — the frontend supplies it so
/// it's already localized.
#[tauri::command]
pub fn applock_unlock_with_biometric(
    session: State<'_, Arc<SessionState>>,
    reason: String,
) -> Result<(), AppError> {
    applock::unlock_with_biometric(session.inner(), &reason)
}

#[tauri::command]
pub fn applock_lock(session: State<'_, Arc<SessionState>>) {
    applock::lock(session.inner());
}

#[tauri::command]
pub fn applock_change_password(
    session: State<'_, Arc<SessionState>>,
    old_password: String,
    new_password: String,
) -> Result<(), AppError> {
    applock::change_password(
        session.inner(),
        Zeroizing::new(old_password),
        Zeroizing::new(new_password),
    )
}

/// Recovery flow. Triggers the OS identity prompt (device password / PIN /
/// passkey / biometric); on success installs `new_password` and unlocks.
#[tauri::command]
pub fn applock_recovery_unlock(
    session: State<'_, Arc<SessionState>>,
    new_password: String,
    reason: String,
) -> Result<(), AppError> {
    applock::recovery_unlock(session.inner(), Zeroizing::new(new_password), &reason)
}

#[tauri::command]
pub fn applock_get_settings() -> Result<LockSettings, AppError> {
    applock::get_lock_settings()
}

#[tauri::command]
pub fn applock_set_settings(settings: LockSettings) -> Result<(), AppError> {
    applock::set_lock_settings(settings)
}

/// Verify a password without changing session state. Used by the change-
/// password and enable-biometric flows that need to re-confirm presence
/// without consuming a rate-limit slot. Returns `true` on match.
#[tauri::command]
pub fn applock_verify_password(password: String) -> Result<bool, AppError> {
    applock::verify_password(Zeroizing::new(password))
}

// --- AWS auth (Contract 03) ---------------------------------------------
//
// These commands wrap the `auth` module. They accept and return plain
// serializable structs; no AWS SDK type and no credential-bearing type
// ever crosses the IPC boundary. The auth module's typed `AuthError` is
// converted to `AppError` here so its stable code reaches the frontend.

#[tauri::command]
pub fn auth_list_profiles() -> Result<Vec<ProfileInfo>, AppError> {
    auth::list_profiles().map_err(AppError::from)
}

#[tauri::command]
pub async fn auth_get_caller_identity(profile: String) -> Result<CallerIdentity, AppError> {
    auth::get_caller_identity(&profile)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
pub async fn auth_test_profile(profile: String) -> Result<ProfileTestResult, AppError> {
    auth::test_profile(&profile).await.map_err(AppError::from)
}

/// PR #66 — write a new AWS CLI profile to `~/.aws/credentials` and
/// `~/.aws/config`. The secret access key arrives inside `input`,
/// is forwarded to `auth::create_profile`, and is dropped from the
/// process when the function returns. No caching, no logging.
#[tauri::command]
pub fn auth_create_profile(input: auth::AddAwsProfileInput) -> Result<String, AppError> {
    auth::create_profile(input).map_err(AppError::from)
}

// --- Multi-account (Contract 04) -----------------------------------------
//
// Every command validates inputs in the `accounts` module before touching
// SQLite. Add/update are async because they verify the profile via STS
// before writing; the rest are synchronous SQLite calls.
//
// Account IDs are returned in full (Contract 04 §Constraints: "masked by
// default in the UI"). The frontend masks unless `reveal_full_ids` is on;
// backend logs (added by later contracts) mask regardless.

#[tauri::command]
pub fn accounts_list() -> Result<Vec<Account>, AppError> {
    accounts::list_accounts().map_err(AppError::from)
}

#[tauri::command]
pub fn accounts_get(aws_account_id: String) -> Result<Account, AppError> {
    accounts::get_account(&aws_account_id).map_err(AppError::from)
}

#[tauri::command]
pub async fn accounts_add(input: AddAccountInput) -> Result<Account, AppError> {
    accounts::add_account(input).await.map_err(AppError::from)
}

#[tauri::command]
pub async fn accounts_update(input: UpdateAccountInput) -> Result<Account, AppError> {
    accounts::update_account(input)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
pub fn accounts_remove(aws_account_id: String) -> Result<RemovalImpact, AppError> {
    accounts::remove_account(&aws_account_id).map_err(AppError::from)
}

#[tauri::command]
pub fn accounts_get_active() -> Result<Option<String>, AppError> {
    accounts::get_active_account().map_err(AppError::from)
}

/// `aws_account_id = None` clears the active selection — the only way a
/// caller can "deselect" without removing the row.
#[tauri::command]
pub fn accounts_set_active(aws_account_id: Option<String>) -> Result<(), AppError> {
    accounts::set_active_account(aws_account_id.as_deref()).map_err(AppError::from)
}

#[tauri::command]
pub fn accounts_get_display_settings() -> Result<AccountsDisplaySettings, AppError> {
    accounts::get_display_settings().map_err(AppError::from)
}

#[tauri::command]
pub fn accounts_set_display_settings(settings: AccountsDisplaySettings) -> Result<(), AppError> {
    accounts::set_display_settings(settings).map_err(AppError::from)
}

// --- Scanner-role connect flow (Phase 2 — replaces Terraform Contract 05) --
//
// `scanner_role_requirements` is async: it calls `sts:GetCallerIdentity` on
// the account's profile to surface the live caller ARN that the user's role
// trust policy must reference. `scanner_role_connect` is async because the
// dry-run `sts:AssumeRole` it performs is an AWS API call. `scanner_role_status`
// is synchronous — pure SQLite read of the existing accounts row.
//
// Inputs are validated inside the `scanner_role` module: account IDs are
// looked up via `accounts::get` (rejects unknown IDs), role ARNs are parsed
// and the embedded account portion checked against the configured account
// before any AWS call, and the trust-policy principal is derived from STS
// (never frontend-typed).

#[tauri::command]
pub async fn scanner_role_requirements(
    aws_account_id: String,
) -> Result<RoleRequirements, AppError> {
    scanner_role::requirements(&aws_account_id)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
pub async fn scanner_role_connect(
    aws_account_id: String,
    role_arn: String,
    policy_variant: PolicyVariant,
) -> Result<ConnectResult, AppError> {
    scanner_role::connect(&aws_account_id, &role_arn, policy_variant)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
pub fn scanner_role_status(aws_account_id: String) -> Result<ProvisioningStatus, AppError> {
    scanner_role::status(&aws_account_id).map_err(AppError::from)
}

// --- Scanner orchestrator (Contract 06) ----------------------------------
//
// `scanner_detect` is synchronous: it only inspects the bundled ScoutSuite
// binary and runs the SHA-256 integrity check. `scanner_run_scan` is async
// because it consults the accounts table (sync) and then dispatches a
// background worker. Progress is exposed via polling (`scanner_scan_status`)
// rather than a live IPC stream — Contract 06 §Constraints.
//
// Account IDs are validated inside the `scanner` module before they become
// path segments or partition keys. The frontend never passes credential
// material across this boundary.

#[tauri::command]
pub fn scanner_detect() -> Result<ScoutSuiteAvailability, AppError> {
    Ok(scanner::detect_binary())
}

#[tauri::command]
pub async fn scanner_run_scan(aws_account_id: String) -> Result<ScanRecord, AppError> {
    scanner::run_scan(&aws_account_id)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
pub fn scanner_scan_status(scan_id: String) -> Result<ScanRecord, AppError> {
    scanner::scan_status(&scan_id).map_err(AppError::from)
}

#[tauri::command]
pub fn scanner_cancel_scan(scan_id: String) -> Result<ScanRecord, AppError> {
    scanner::cancel_scan(&scan_id).map_err(AppError::from)
}

/// Open the platform file manager at the scan's output directory so the
/// user can inspect `raw-scout.json`, `scoutsuite-stderr.log`, and the
/// `scoutsuite-results/` tree. Exposed in the UI from the failure banner
/// in `ScanProgress.tsx` so a failed scan is self-serve to triage — the
/// scoutsuite stderr that pinned the macOS hardened-runtime regression
/// in 2026.5.9-2026.5.12 lived in a Finder-hidden `~/Library` path and
/// took 10+ minutes to even locate. This command removes that friction.
#[tauri::command]
pub fn scanner_reveal_scan_dir(scan_id: String) -> Result<(), AppError> {
    scanner::reveal_scan_dir(&scan_id).map_err(AppError::from)
}

#[tauri::command]
pub fn scanner_list_recent(
    aws_account_id: String,
    limit: Option<usize>,
) -> Result<Vec<ScanRecord>, AppError> {
    scanner::list_recent_scans(&aws_account_id, limit.unwrap_or(20)).map_err(AppError::from)
}

// --- Findings parser & store (Contract 07) --------------------------------
//
// Wrappers around the `findings` module. Like the scanner namespace these
// are mostly synchronous SQLite calls; `findings_parse_and_store` is sync
// too because the parse is local-only (no network, no STS) and ScoutSuite
// outputs land in the low-MB range — well within a single IPC dispatch.
//
// Every command validates inputs inside the `findings` module before any
// SQL runs. The frontend never crosses this boundary with credentials.

#[tauri::command]
pub fn findings_parse_and_store(scan_id: String) -> Result<ParseSummary, AppError> {
    findings::parse_and_store(&scan_id).map_err(AppError::from)
}

#[tauri::command]
pub fn findings_list(
    scan_id: String,
    filter: Option<FindingsFilter>,
) -> Result<Vec<Finding>, AppError> {
    findings::list_findings(&scan_id, filter.unwrap_or_default()).map_err(AppError::from)
}

#[tauri::command]
pub fn findings_get(finding_id: String) -> Result<FindingDetail, AppError> {
    findings::get_finding(&finding_id).map_err(AppError::from)
}

#[tauri::command]
pub fn findings_list_scans(aws_account_id: String) -> Result<Vec<ScanRecord>, AppError> {
    findings::list_scans(&aws_account_id).map_err(AppError::from)
}

#[tauri::command]
pub fn findings_get_scan(scan_id: String) -> Result<ScanRecord, AppError> {
    findings::get_scan(&scan_id).map_err(AppError::from)
}

#[tauri::command]
pub fn findings_delete_scan(scan_id: String) -> Result<DeleteScanImpact, AppError> {
    findings::delete_scan(&scan_id).map_err(AppError::from)
}

// --- Knowledge base & compliance mapping (Contract 08) --------------------
//
// Bundled article + mapping reads are synchronous SQLite / in-memory
// lookups; `kb_check_for_update` and `kb_apply_update` are async because
// they perform a network fetch from a public documentation source. The
// refresh feature is opt-in (default OFF) — every command validates the
// `finding_id` inside the `knowledgebase` module before any work runs.
//
// CLAUDE.md §5 hard-DO-NOT: no account information, finding identifiers,
// or scan data ever crosses the network in a refresh call. The fetch is
// a plain GET of public security documentation.

#[tauri::command]
pub fn kb_get_article(finding_id: String) -> Result<KnowledgeArticle, AppError> {
    knowledgebase::get_article(&finding_id).map_err(AppError::from)
}

#[tauri::command]
pub fn kb_list_articles() -> Result<Vec<ArticleSummary>, AppError> {
    knowledgebase::list_articles().map_err(AppError::from)
}

#[tauri::command]
pub fn kb_get_control_mappings(finding_id: String) -> Result<ControlMapping, AppError> {
    knowledgebase::get_control_mappings(&finding_id).map_err(AppError::from)
}

#[tauri::command]
pub fn kb_list_frameworks() -> Result<Vec<Framework>, AppError> {
    knowledgebase::list_frameworks().map_err(AppError::from)
}

#[tauri::command]
pub fn kb_get_refresh_settings() -> Result<RefreshSettings, AppError> {
    knowledgebase::get_refresh_settings().map_err(AppError::from)
}

#[tauri::command]
pub fn kb_set_refresh_settings(update: RefreshSettingsUpdate) -> Result<RefreshSettings, AppError> {
    knowledgebase::set_refresh_settings(update).map_err(AppError::from)
}

/// Probe for an available KB update. Errors with `kb_refresh_disabled`
/// if the user hasn't opted in. Runs the HTTP fetch on a blocking
/// worker thread because the underlying client is sync.
#[tauri::command]
pub async fn kb_check_for_update() -> Result<RefreshCheckResult, AppError> {
    tokio::task::spawn_blocking(knowledgebase::check_for_kb_update)
        .await
        .map_err(|e| AppError::Internal(format!("kb_check spawn: {e}")))?
        .map_err(AppError::from)
}

/// Fetch + validate + install a remote bundle. On failure the bundled
/// baseline (or any prior remote cache) is preserved untouched.
#[tauri::command]
pub async fn kb_apply_update() -> Result<RefreshApplyResult, AppError> {
    tokio::task::spawn_blocking(knowledgebase::apply_kb_update)
        .await
        .map_err(|e| AppError::Internal(format!("kb_apply spawn: {e}")))?
        .map_err(AppError::from)
}

// --- Scheduled & automated scans (Contract 10) ---------------------------
//
// Every command validates inputs inside the `scheduler` module before any
// SQL runs. Account IDs are re-checked against the 12-digit grammar so a
// malformed string can never become a SQL primary key. Schedules are
// non-secret configuration only — the actual scan still flows through the
// `scanner` namespace, which is where AssumeRole + binary verification
// live (CLAUDE.md §4.3).

#[tauri::command]
pub fn scheduler_set_schedule(input: SetScheduleInput) -> Result<Schedule, AppError> {
    scheduler::set_schedule(input).map_err(AppError::from)
}

#[tauri::command]
pub fn scheduler_get_schedule(aws_account_id: String) -> Result<Schedule, AppError> {
    scheduler::get_schedule(&aws_account_id).map_err(AppError::from)
}

#[tauri::command]
pub fn scheduler_clear_schedule(aws_account_id: String) -> Result<(), AppError> {
    scheduler::clear_schedule(&aws_account_id).map_err(AppError::from)
}

#[tauri::command]
pub fn scheduler_list_schedules() -> Result<Vec<Schedule>, AppError> {
    scheduler::list_schedules().map_err(AppError::from)
}

#[tauri::command]
pub fn scheduler_next_run_times() -> Result<Vec<NextRunTime>, AppError> {
    scheduler::next_run_times().map_err(AppError::from)
}

#[tauri::command]
pub fn scheduler_recent_events(
    aws_account_id: String,
    limit: Option<usize>,
) -> Result<Vec<ScheduleEvent>, AppError> {
    scheduler::recent_events(&aws_account_id, limit.unwrap_or(20)).map_err(AppError::from)
}

// --- Event log, retention, hard delete & panic (Contract 11) -------------
//
// The event log is append-only. Every command here validates its inputs
// inside the underlying module before any write runs. Deletion / panic
// gates use a typed-confirmation string the backend re-checks
// (CLAUDE.md §4.1: every command validates its inputs; the frontend gate
// is convenience, not security).

#[tauri::command]
pub fn eventlog_list(filter: Option<EventLogFilter>) -> Result<Vec<EventLogEntry>, AppError> {
    eventlog::list_events(filter.unwrap_or_default()).map_err(AppError::from)
}

#[tauri::command]
pub fn eventlog_search(query: String, limit: Option<i64>) -> Result<Vec<EventLogEntry>, AppError> {
    eventlog::search_events(&query, limit).map_err(AppError::from)
}

/// Returns the full activity log as newline-delimited JSON. The caller
/// (Settings → Activity Log) writes this to a user-chosen file.
#[tauri::command]
pub fn eventlog_export() -> Result<String, AppError> {
    eventlog::export_events().map_err(AppError::from)
}

/// Clears only the activity-log VIEW. Underlying rows remain subject to
/// the event-log retention policy and still appear in Export.
#[tauri::command]
pub fn eventlog_clear_view() -> Result<(), AppError> {
    eventlog::clear_event_view().map_err(AppError::from)
}

#[tauri::command]
pub fn eventlog_count() -> Result<i64, AppError> {
    eventlog::count_events().map_err(AppError::from)
}

#[tauri::command]
pub fn retention_get_settings() -> Result<RetentionSettings, AppError> {
    retention::get_settings().map_err(AppError::from)
}

#[tauri::command]
pub fn retention_set_scan(period: RetentionPeriod) -> Result<(), AppError> {
    retention::set_scan_retention(period).map_err(AppError::from)
}

#[tauri::command]
pub fn retention_set_eventlog(period: RetentionPeriod) -> Result<(), AppError> {
    retention::set_eventlog_retention(period).map_err(AppError::from)
}

#[tauri::command]
pub fn retention_run_now() -> Result<RetentionRunSummary, AppError> {
    retention::run_now().map_err(AppError::from)
}

/// Hard-delete a scan. Requires the user to have typed either `DELETE`
/// or the full scan ID. Runs the SQLite cascade, unlinks the raw file
/// (and per-scan output directory), then executes `VACUUM` so removed
/// rows are not trivially recoverable.
#[tauri::command]
pub fn deletion_hard_delete_scan(
    scan_id: String,
    confirmation: String,
    options: Option<HardDeleteOptions>,
) -> Result<HardDeleteSummary, AppError> {
    deletion::hard_delete_scan(&scan_id, &confirmation, options.unwrap_or_default())
        .map_err(AppError::from)
}

/// Run `VACUUM` against the SQLite file. Exposed so a tooling script
/// (and the QA contract) can request it without forcing a delete.
#[tauri::command]
pub fn deletion_vacuum_now() -> Result<(), AppError> {
    deletion::run_vacuum().map_err(AppError::from)
}

/// Panic — wipe every CloudSaw trace on this machine. The data wipe is
/// IMMEDIATE and SYNCHRONOUS. Requires the literal confirmation string
/// `"PANIC"`. The two-phase app/installer self-delete is staged via a
/// platform-specific helper; the data wipe still succeeds if staging
/// fails.
#[tauri::command]
pub fn system_panic_wipe(confirmation: String) -> Result<PanicWipeResult, AppError> {
    wipe::run_panic_wipe(&confirmation)
}

/// Reboot the machine at user-level. Only called after the user picks
/// "Reboot now" in the post-panic dialog — "Later" never calls this.
#[tauri::command]
pub fn system_request_reboot() -> Result<(), AppError> {
    wipe::selfdelete::request_user_reboot().map_err(|e| AppError::Io(format!("reboot: {e}")))
}

// --- GitHub integration (Contract 12) ------------------------------------
//
// Two surfaces share one PAT (stored only in the OS keychain at
// `cloudsaw.github_pat`). The IPC bridge never accepts a token in plain
// IPC payloads other than `github_set_token`; every other call reads
// the PAT from the keychain inside the underlying module. Direct API
// submission always goes through the `prepare → preview → submit`
// dance — the UI shows the preview to the user before submission
// (Contract 12 §Constraints).

#[tauri::command]
pub fn github_get_settings() -> Result<GithubSettings, AppError> {
    github::get_settings().map_err(AppError::from)
}

#[tauri::command]
pub fn github_set_token(token: String) -> Result<(), AppError> {
    github::set_token(token).map_err(AppError::from)
}

#[tauri::command]
pub fn github_clear_token() -> Result<(), AppError> {
    github::clear_token().map_err(AppError::from)
}

#[tauri::command]
pub fn github_set_findings_repo(repo: Option<RepoSelection>) -> Result<(), AppError> {
    github::set_findings_repo(repo).map_err(AppError::from)
}

/// URL of the GitHub fine-grained-token settings page. Returned to the
/// frontend so the "Generate token" button opens it via the OS browser.
#[tauri::command]
pub fn github_generate_token_url() -> String {
    github::generate_token_url().to_string()
}

#[tauri::command]
pub async fn github_prepare_error_report(
    notes: Option<String>,
    locale: String,
) -> Result<IssuePreview, AppError> {
    tokio::task::spawn_blocking(move || github::prepare_error_report(notes, &locale))
        .await
        .map_err(|e| AppError::Internal(format!("github_prepare_error spawn: {e}")))?
        .map_err(AppError::from)
}

#[tauri::command]
pub async fn github_submit_error_report(preview: IssuePreview) -> Result<IssueCreated, AppError> {
    tokio::task::spawn_blocking(move || github::submit_error_report(&preview))
        .await
        .map_err(|e| AppError::Internal(format!("github_submit_error spawn: {e}")))?
        .map_err(AppError::from)
}

#[tauri::command]
pub fn github_browser_fallback_for_error(
    preview: IssuePreview,
) -> Result<BrowserSubmission, AppError> {
    Ok(github::browser_fallback_for_error_report(&preview))
}

#[tauri::command]
pub fn github_prepare_finding_ticket(
    finding_id: String,
    repo: RepoSelection,
) -> Result<IssuePreview, AppError> {
    github::prepare_finding_ticket(&finding_id, &repo).map_err(AppError::from)
}

#[tauri::command]
pub async fn github_submit_finding_ticket(
    finding_id: String,
    preview: IssuePreview,
) -> Result<FindingTicket, AppError> {
    tokio::task::spawn_blocking(move || github::submit_finding_ticket(&finding_id, &preview))
        .await
        .map_err(|e| AppError::Internal(format!("github_submit_finding spawn: {e}")))?
        .map_err(AppError::from)
}

#[tauri::command]
pub fn github_browser_fallback_for_finding(
    preview: IssuePreview,
) -> Result<BrowserSubmission, AppError> {
    Ok(github::browser_fallback_for_finding_ticket(&preview))
}

#[tauri::command]
pub fn github_get_finding_ticket(finding_id: String) -> Result<Option<FindingTicket>, AppError> {
    github::get_finding_ticket(&finding_id).map_err(AppError::from)
}

#[tauri::command]
pub fn github_list_finding_tickets(aws_account_id: String) -> Result<Vec<FindingTicket>, AppError> {
    github::list_finding_tickets(&aws_account_id).map_err(AppError::from)
}

// --- AI Suggestion Layer (Contract 13) -----------------------------------
//
// Fully OPT-IN: with no provider key configured, no AI command makes any
// network call. The bridge here re-checks every gate inside the
// underlying module so a direct IPC caller can never bypass the
// "must preview before send" rule (CLAUDE.md §4.1).

#[tauri::command]
pub fn ai_get_settings() -> Result<AiSettings, AppError> {
    ai::get_settings().map_err(AppError::from)
}

#[tauri::command]
pub fn ai_set_provider(provider: Option<Provider>) -> Result<(), AppError> {
    ai::set_provider(provider).map_err(AppError::from)
}

#[tauri::command]
pub fn ai_set_provider_key(provider: Provider, key: String) -> Result<(), AppError> {
    ai::set_provider_key(provider, key).map_err(AppError::from)
}

#[tauri::command]
pub fn ai_clear_provider_key(provider: Provider) -> Result<(), AppError> {
    ai::clear_provider_key(provider).map_err(AppError::from)
}

#[tauri::command]
pub fn ai_has_provider_key(provider: Provider) -> Result<bool, AppError> {
    ai::has_provider_key(provider).map_err(AppError::from)
}

#[tauri::command]
pub fn ai_set_business_context(context: BusinessContext) -> Result<(), AppError> {
    ai::set_business_context(context).map_err(AppError::from)
}

/// Build the request preview the UI MUST display to the user before
/// any `ai_send_request` call. The returned value IS the payload that
/// would be transmitted.
#[tauri::command]
pub fn ai_prepare_request(finding_id: String) -> Result<AiRequestPreview, AppError> {
    ai::prepare_request(&finding_id).map_err(AppError::from)
}

/// Send the previously-built preview to the connected provider.
/// Spawns on a blocking worker because the underlying transport is
/// sync (`reqwest::blocking`).
#[tauri::command]
pub async fn ai_send_request(preview: AiRequestPreview) -> Result<AiSuggestion, AppError> {
    tokio::task::spawn_blocking(move || ai::send_request(preview))
        .await
        .map_err(|e| AppError::Internal(format!("ai_send spawn: {e}")))?
        .map_err(AppError::from)
}

// --- Onboarding wizard (Contract 14) ------------------------------------
//
// The wizard is the only entry point on first launch (App.tsx gates the
// main app behind `onboarding_get_state().completed`). Every IPC here
// just touches the singleton onboarding row — credentials, account
// identifiers, and the master password ALL live in their owning
// modules (applock / accounts / terraform / scanner / ai). The wizard
// itself stores only step flags + language.

#[tauri::command]
pub fn onboarding_get_state() -> Result<OnboardingState, AppError> {
    onboarding::get_state().map_err(AppError::from)
}

#[tauri::command]
pub fn onboarding_set_language(language: String) -> Result<(), AppError> {
    onboarding::set_language(&language).map_err(AppError::from)
}

#[tauri::command]
pub fn onboarding_set_current_step(step: OnboardingStep) -> Result<(), AppError> {
    onboarding::set_current_step(step).map_err(AppError::from)
}

#[tauri::command]
pub fn onboarding_mark_step_completed(step: OnboardingStep) -> Result<(), AppError> {
    onboarding::mark_step_completed(step).map_err(AppError::from)
}

#[tauri::command]
pub fn onboarding_complete() -> Result<(), AppError> {
    onboarding::complete().map_err(AppError::from)
}

/// Reset the wizard for a Settings-driven re-run. `start_at` lets the
/// caller jump straight to a specific step (typically `aws_account` for
/// "add another account").
#[tauri::command]
pub fn onboarding_reset_for_rerun(start_at: OnboardingStep) -> Result<(), AppError> {
    onboarding::reset_for_rerun(start_at).map_err(AppError::from)
}

// --- Report exporter (Contract 15) ---------------------------------------
//
// The frontend MUST source `output_path` from `dialog.save()` — the
// Rust side validates the shape (non-empty, has a parent that exists,
// not directory-ended). PDF + HTML are split into separate IPCs so
// the UI can offer the user the chosen format directly. Async because
// PDF generation is CPU-bound and we don't want to block the tokio
// runtime that hosts other IPCs.

#[tauri::command]
pub async fn report_export_scan_html(
    scan_id: String,
    output_path: String,
    disclosure: AccountIdDisclosure,
) -> Result<ExportOutcome, AppError> {
    tokio::task::spawn_blocking(move || {
        reports::export_scan_html(&scan_id, &output_path, disclosure)
    })
    .await
    .map_err(|e| AppError::Internal(format!("report_export_scan_html spawn: {e}")))?
    .map_err(AppError::from)
}

#[tauri::command]
pub async fn report_export_scan_pdf(
    scan_id: String,
    output_path: String,
    disclosure: AccountIdDisclosure,
) -> Result<ExportOutcome, AppError> {
    tokio::task::spawn_blocking(move || {
        reports::export_scan_pdf(&scan_id, &output_path, disclosure)
    })
    .await
    .map_err(|e| AppError::Internal(format!("report_export_scan_pdf spawn: {e}")))?
    .map_err(AppError::from)
}

#[tauri::command]
pub async fn report_export_custom_html(
    start: String,
    end: String,
    account_scope: Vec<String>,
    output_path: String,
    disclosure: AccountIdDisclosure,
) -> Result<ExportOutcome, AppError> {
    let (start_dt, end_dt) = parse_range(&start, &end)?;
    tokio::task::spawn_blocking(move || {
        reports::export_custom_html(start_dt, end_dt, &account_scope, &output_path, disclosure)
    })
    .await
    .map_err(|e| AppError::Internal(format!("report_export_custom_html spawn: {e}")))?
    .map_err(AppError::from)
}

#[tauri::command]
pub async fn report_export_custom_pdf(
    start: String,
    end: String,
    account_scope: Vec<String>,
    output_path: String,
    disclosure: AccountIdDisclosure,
) -> Result<ExportOutcome, AppError> {
    let (start_dt, end_dt) = parse_range(&start, &end)?;
    tokio::task::spawn_blocking(move || {
        reports::export_custom_pdf(start_dt, end_dt, &account_scope, &output_path, disclosure)
    })
    .await
    .map_err(|e| AppError::Internal(format!("report_export_custom_pdf spawn: {e}")))?
    .map_err(AppError::from)
}

#[tauri::command]
pub fn report_preview_scan(
    scan_id: String,
    disclosure: AccountIdDisclosure,
) -> Result<ReportContent, AppError> {
    reports::preview_scan(&scan_id, disclosure).map_err(AppError::from)
}

#[tauri::command]
pub fn report_preview_custom(
    start: String,
    end: String,
    account_scope: Vec<String>,
    disclosure: AccountIdDisclosure,
) -> Result<ReportContent, AppError> {
    let (start_dt, end_dt) = parse_range(&start, &end)?;
    reports::preview_custom(start_dt, end_dt, &account_scope, disclosure).map_err(AppError::from)
}

#[tauri::command]
pub fn report_get_settings() -> Result<ReportSettings, AppError> {
    reports::get_settings().map_err(AppError::from)
}

#[tauri::command]
pub fn report_set_settings(settings: ReportSettings) -> Result<(), AppError> {
    reports::set_settings(settings).map_err(AppError::from)
}

fn parse_range(
    start: &str,
    end: &str,
) -> Result<(chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>), AppError> {
    let start_dt = chrono::DateTime::parse_from_rfc3339(start)
        .map_err(|_| AppError::InvalidInput("start".into()))?
        .with_timezone(&chrono::Utc);
    let end_dt = chrono::DateTime::parse_from_rfc3339(end)
        .map_err(|_| AppError::InvalidInput("end".into()))?
        .with_timezone(&chrono::Utc);
    Ok((start_dt, end_dt))
}
