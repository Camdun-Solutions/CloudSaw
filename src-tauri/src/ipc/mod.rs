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

use crate::applock::{self, LockSettings, LockState, SessionState};
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
