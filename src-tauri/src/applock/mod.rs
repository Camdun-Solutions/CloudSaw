// App-lock module — Contract 02.
//
// The master password is a **UI lock only**, not an encryption key. It gates
// access to the app window; it does NOT encrypt anything at rest. See
// Contract C02-app-lock.md and CLAUDE.md §5.
//
// Public surface (mirrors the contract's "Expected Output"):
//
//     set_master_password(password)              first-run only
//     verify_password(password) -> bool          read-only check
//     change_password(old, new)                  atomic swap
//     is_locked() -> bool                        in-memory session state
//     unlock(password)                           password unlock
//     unlock_with_biometric()                    biometric unlock
//     lock()                                     manual lock
//     recovery_unlock(new_password)              after OS identity verify
//     get_lock_settings() -> LockSettings
//     set_lock_settings(LockSettings)
//
// All private helpers (storage layout, hashing parameters, session-state
// shape) are deliberately not re-exported — the contract specifies behavior
// and public shape, not internals.

use std::sync::Arc;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::errors::AppError;

pub mod biometric;
mod hashing;
pub mod identity;
mod session;
mod storage;

pub use session::SessionState;

// --- Public types --------------------------------------------------------

/// Re-lock cadence. Encoded uniformly across SQLite, IPC, and the UI so we
/// don't have three almost-equivalent shapes to keep in sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "seconds")]
pub enum LockPeriod {
    /// Lock immediately when the last window closes; next launch always
    /// re-prompts.
    Immediate,
    /// Re-prompt after `n` seconds of inactivity.
    After(u64),
    /// Don't re-prompt until the user manually locks.
    Never,
}

impl LockPeriod {
    fn to_storage(self) -> Option<i64> {
        match self {
            LockPeriod::Immediate => Some(0),
            LockPeriod::After(s) => Some(s as i64),
            LockPeriod::Never => None,
        }
    }

    fn from_storage(v: Option<i64>) -> LockPeriod {
        match v {
            None => LockPeriod::Never,
            Some(0) => LockPeriod::Immediate,
            Some(n) if n > 0 => LockPeriod::After(n as u64),
            // Negative would be a corrupted row; treat conservatively.
            Some(_) => LockPeriod::Immediate,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockSettings {
    pub lock_period: LockPeriod,
    pub biometric_enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LockState {
    /// True until a password has been set.
    pub first_run: bool,
    /// True if the current process needs the user to authenticate before the
    /// main UI is shown.
    pub locked: bool,
    pub settings: LockSettings,
    pub biometric_availability: biometric::Availability,
    pub recovery_available: bool,
}

// --- Bootstrap (called from lib::run) ------------------------------------

/// Decide the initial in-process lock state at app launch. Consumes the
/// stored `last_unlocked_at` and the configured period to produce a verdict.
pub fn bootstrap_session() -> Result<Arc<SessionState>, AppError> {
    let state = Arc::new(SessionState::new());
    let row = storage::read()?;
    let period = LockPeriod::from_storage(row.lock_period_seconds);
    let initially_unlocked = match (row.password_hash.as_deref(), period, row.last_unlocked_at) {
        // No password yet → not locked; the first-run UI takes over.
        (None, _, _) => false,
        // immediate-on-close: every launch re-prompts.
        (Some(_), LockPeriod::Immediate, _) => false,
        // never-relock: unlocked iff we've ever unlocked before.
        (Some(_), LockPeriod::Never, Some(_)) => true,
        (Some(_), LockPeriod::Never, None) => false,
        // timed: unlocked iff still within the window.
        (Some(_), LockPeriod::After(secs), Some(last)) => {
            let elapsed = Utc::now().signed_duration_since(last);
            elapsed.num_seconds() >= 0 && (elapsed.num_seconds() as u64) < secs
        }
        (Some(_), LockPeriod::After(_), None) => false,
    };
    if initially_unlocked {
        state.mark_unlocked();
    }
    Ok(state)
}

// --- Validation helpers --------------------------------------------------

const MIN_PASSWORD_LEN: usize = 8;
const MAX_PASSWORD_LEN: usize = 1024;

fn validate_new_password(pw: &str) -> Result<(), AppError> {
    let len = pw.chars().count();
    if len < MIN_PASSWORD_LEN {
        return Err(AppError::InvalidInput("password too short".into()));
    }
    if len > MAX_PASSWORD_LEN {
        return Err(AppError::InvalidInput("password too long".into()));
    }
    Ok(())
}

// --- Public API ---------------------------------------------------------

pub fn get_state(session: &SessionState) -> Result<LockState, AppError> {
    let row = storage::read()?;
    let first_run = row.password_hash.is_none();
    let settings = LockSettings {
        lock_period: LockPeriod::from_storage(row.lock_period_seconds),
        biometric_enabled: row.biometric_enabled,
    };
    let locked = !first_run && !session.is_unlocked();
    Ok(LockState {
        first_run,
        locked,
        settings,
        biometric_availability: biometric::availability(),
        recovery_available: identity::is_available(),
    })
}

pub fn get_lock_settings() -> Result<LockSettings, AppError> {
    let row = storage::read()?;
    Ok(LockSettings {
        lock_period: LockPeriod::from_storage(row.lock_period_seconds),
        biometric_enabled: row.biometric_enabled,
    })
}

/// First-run setup. Refuses if a password is already configured — the change
/// flow is the only path to overwrite an existing hash.
pub fn set_master_password(
    session: &SessionState,
    password: Zeroizing<String>,
) -> Result<(), AppError> {
    validate_new_password(&password)?;
    let row = storage::read()?;
    if row.password_hash.is_some() {
        return Err(AppError::AlreadyConfigured);
    }
    let phc = hashing::hash_password(&password)?;
    storage::set_password_hash(Some(&phc))?;
    let now = Utc::now();
    storage::record_unlock(now)?;
    session.mark_unlocked();
    Ok(())
}

/// Read-only verification. Does NOT mutate session state and does NOT touch
/// the rate limiter. Useful for re-confirming the password inside protected
/// flows (e.g. enabling biometric).
pub fn verify_password(password: Zeroizing<String>) -> Result<bool, AppError> {
    let row = storage::read()?;
    let phc = row.password_hash.ok_or(AppError::NotConfigured)?;
    hashing::verify_password(&phc, &password)
}

pub fn change_password(
    session: &SessionState,
    old: Zeroizing<String>,
    new: Zeroizing<String>,
) -> Result<(), AppError> {
    validate_new_password(&new)?;
    // Re-hash up front so a hash failure doesn't leave us between two states.
    let new_phc = hashing::hash_password(&new)?;
    storage::replace_password_hash_atomic(
        |stored| hashing::verify_password(stored, &old),
        &new_phc,
    )?;
    // Treat a successful change as a fresh unlock — it just proved presence.
    let now = Utc::now();
    storage::record_unlock(now)?;
    session.mark_unlocked();
    Ok(())
}

pub fn is_locked(session: &SessionState) -> bool {
    !session.is_unlocked()
}

pub fn lock(session: &SessionState) {
    session.mark_locked();
}

/// Password unlock. Returns `Ok(())` on success; `PasswordRejected` on
/// mismatch; `RateLimited(seconds)` if the user is currently in backoff.
/// All three failure paths reveal the same amount of information — the
/// caller cannot tell a "close" password from a totally wrong one.
pub fn unlock(session: &SessionState, password: Zeroizing<String>) -> Result<(), AppError> {
    if let Some(remaining) = session.check_backoff() {
        return Err(session::backoff_to_error(remaining));
    }
    let row = storage::read()?;
    let phc = row.password_hash.ok_or(AppError::NotConfigured)?;
    match hashing::verify_password(&phc, &password)? {
        true => {
            storage::record_unlock(Utc::now())?;
            session.mark_unlocked();
            Ok(())
        }
        false => {
            let delay = session.record_failure();
            if delay.is_zero() {
                Err(AppError::PasswordRejected)
            } else {
                Err(session::backoff_to_error(delay))
            }
        }
    }
}

/// Biometric unlock. Only succeeds when:
///   * biometric is enabled in settings,
///   * the platform reports biometric available,
///   * the user passes the OS biometric prompt.
pub fn unlock_with_biometric(session: &SessionState, reason: &str) -> Result<(), AppError> {
    if let Some(remaining) = session.check_backoff() {
        return Err(session::backoff_to_error(remaining));
    }
    let row = storage::read()?;
    if row.password_hash.is_none() {
        return Err(AppError::NotConfigured);
    }
    if !row.biometric_enabled {
        return Err(AppError::BiometricUnavailable);
    }
    match biometric::prompt(reason)? {
        biometric::Verdict::Verified => {
            storage::record_unlock(Utc::now())?;
            session.mark_unlocked();
            Ok(())
        }
        biometric::Verdict::Declined => {
            // A declined biometric prompt is a presence failure, not a wrong
            // password — count it for backoff so a hostile user can't spam
            // the prompt either.
            let delay = session.record_failure();
            if delay.is_zero() {
                Err(AppError::PasswordRejected)
            } else {
                Err(session::backoff_to_error(delay))
            }
        }
    }
}

/// Recovery flow. The caller MUST have already passed the OS identity prompt
/// (we re-check here so a malicious IPC client can't skip it). On success,
/// installs `new_password` as the master password and unlocks the session.
///
/// Recovery never reveals or returns the old password (Contract 02 constraint).
pub fn recovery_unlock(
    session: &SessionState,
    new_password: Zeroizing<String>,
    reason: &str,
) -> Result<(), AppError> {
    validate_new_password(&new_password)?;
    let row = storage::read()?;
    if row.password_hash.is_none() {
        // Recovery before first-run is undefined — fall back to first-run
        // setup rather than silently treating recovery as setup.
        return Err(AppError::NotConfigured);
    }
    match identity::prompt(reason)? {
        identity::Verdict::Verified => {
            let phc = hashing::hash_password(&new_password)?;
            storage::set_password_hash(Some(&phc))?;
            storage::record_unlock(Utc::now())?;
            session.mark_unlocked();
            Ok(())
        }
        identity::Verdict::Declined => {
            // Failed identity check counts toward backoff for the same
            // reason as a declined biometric.
            let delay = session.record_failure();
            if delay.is_zero() {
                Err(AppError::PasswordRejected)
            } else {
                Err(session::backoff_to_error(delay))
            }
        }
    }
}

pub fn set_lock_settings(settings: LockSettings) -> Result<(), AppError> {
    // Refuse to enable biometric on a platform that doesn't expose it.
    if settings.biometric_enabled
        && !matches!(
            biometric::availability(),
            biometric::Availability::Available
        )
    {
        return Err(AppError::BiometricUnavailable);
    }
    storage::set_lock_period_seconds(settings.lock_period.to_storage())?;
    storage::set_biometric_enabled(settings.biometric_enabled)?;
    Ok(())
}
