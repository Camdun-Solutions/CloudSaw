// IPC surface. Every `#[tauri::command]` declared here MUST:
//   - validate its inputs (no command trusts a value from the frontend)
//   - return `Result<T, AppError>` (or an infallible primitive)
//   - never accept or return credential-bearing types
//
// IPC payloads use plain serializable structs. AWS SDK types never cross this
// boundary. See CLAUDE.md §4.1.

use std::sync::Arc;

use tauri::State;
use zeroize::Zeroizing;

use crate::accounts::{
    self, Account, AccountsDisplaySettings, AddAccountInput, RemovalImpact, UpdateAccountInput,
};
use crate::applock::{self, LockSettings, LockState, SessionState};
use crate::auth::{self, CallerIdentity, ProfileInfo, ProfileTestResult};
use crate::errors::AppError;

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
    accounts::update_account(input).await.map_err(AppError::from)
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
pub fn accounts_set_display_settings(
    settings: AccountsDisplaySettings,
) -> Result<(), AppError> {
    accounts::set_display_settings(settings).map_err(AppError::from)
}
