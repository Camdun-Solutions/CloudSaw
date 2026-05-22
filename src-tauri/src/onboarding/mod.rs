// Onboarding wizard — Contract 14.
//
// On first launch, the main app is unreachable until the wizard
// completes (App.tsx gates on `state.completed`). The wizard is
// resumable: every step-completion flag and the chosen language live
// in a single SQLite row (migration 0011). On relaunch, the IPC
// returns the same row and the UI re-renders at `current_step`.
//
// Contract 14 §Constraints (carved into the storage shape itself):
//   * The wizard stores NO credentials and NO account-identifying
//     information. Its own state is just six step flags + language.
//   * No step auto-advances. The IPC accepts a `set_current_step`
//     call but each transition is driven by an explicit UI action.
//   * After completion the `completed` flag is set; subsequent
//     launches read it and route straight to the main app.

pub mod error;

pub use error::OnboardingError;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::db::paths::app_data_dir;

/// The six steps the wizard exposes, matching Contract 14's Expected
/// Output. The `Done` variant represents post-completion and is not a
/// real step — it exists so the frontend can read it back as a
/// terminal state without a sentinel value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnboardingStep {
    Language,
    MasterPassword,
    AwsAccount,
    Terraform,
    BusinessContext,
    FirstScan,
    Done,
}

impl OnboardingStep {
    pub fn from_index(i: i64) -> Self {
        match i {
            1 => OnboardingStep::Language,
            2 => OnboardingStep::MasterPassword,
            3 => OnboardingStep::AwsAccount,
            4 => OnboardingStep::Terraform,
            5 => OnboardingStep::BusinessContext,
            6 => OnboardingStep::FirstScan,
            _ => OnboardingStep::Done,
        }
    }
    pub fn to_index(self) -> i64 {
        match self {
            OnboardingStep::Language => 1,
            OnboardingStep::MasterPassword => 2,
            OnboardingStep::AwsAccount => 3,
            OnboardingStep::Terraform => 4,
            OnboardingStep::BusinessContext => 5,
            OnboardingStep::FirstScan => 6,
            OnboardingStep::Done => 7,
        }
    }
}

/// Snapshot of the onboarding state, returned to the frontend on every
/// IPC read.
#[derive(Debug, Clone, Serialize)]
pub struct OnboardingState {
    pub completed: bool,
    pub current_step: OnboardingStep,
    pub language: String,
    pub step_language_completed: bool,
    pub step_password_completed: bool,
    pub step_account_completed: bool,
    pub step_terraform_completed: bool,
    pub step_context_completed: bool,
    pub step_first_scan_completed: bool,
    pub completed_at: Option<DateTime<Utc>>,
}

fn db_path() -> Result<std::path::PathBuf, OnboardingError> {
    Ok(app_data_dir()
        .map_err(|e| OnboardingError::Db(e.to_string()))?
        .join("db")
        .join("cloudsaw.db"))
}

fn open() -> Result<Connection, OnboardingError> {
    Connection::open(db_path()?).map_err(OnboardingError::from)
}

pub fn get_state() -> Result<OnboardingState, OnboardingError> {
    let conn = open()?;
    let row = conn
        .query_row(
            "SELECT completed, current_step, language,
                    step_language_completed, step_password_completed,
                    step_account_completed, step_terraform_completed,
                    step_context_completed, step_first_scan_completed,
                    completed_at
               FROM onboarding_state WHERE id = 1",
            [],
            |r| {
                Ok((
                    r.get::<_, i64>(0)? != 0,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)? != 0,
                    r.get::<_, i64>(4)? != 0,
                    r.get::<_, i64>(5)? != 0,
                    r.get::<_, i64>(6)? != 0,
                    r.get::<_, i64>(7)? != 0,
                    r.get::<_, i64>(8)? != 0,
                    r.get::<_, Option<String>>(9)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| OnboardingError::Db("onboarding_state row missing".into()))?;

    let completed_at = match row.9 {
        Some(s) if !s.is_empty() => Some(
            DateTime::parse_from_rfc3339(&s)
                .map_err(|e| OnboardingError::Db(format!("bad completed_at: {e}")))?
                .with_timezone(&Utc),
        ),
        _ => None,
    };

    Ok(OnboardingState {
        completed: row.0,
        current_step: OnboardingStep::from_index(row.1),
        language: row.2,
        step_language_completed: row.3,
        step_password_completed: row.4,
        step_account_completed: row.5,
        step_terraform_completed: row.6,
        step_context_completed: row.7,
        step_first_scan_completed: row.8,
        completed_at,
    })
}

/// Persist the language pick. Validates against the supported set so
/// the wizard's stored language matches the in-memory locale dictionary.
pub fn set_language(language: &str) -> Result<(), OnboardingError> {
    if !matches!(language, "en" | "es" | "fr" | "zh") {
        return Err(OnboardingError::InvalidInput("language"));
    }
    let conn = open()?;
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE onboarding_state
            SET language = ?1, updated_at = ?2
          WHERE id = 1",
        params![language, now],
    )?;
    Ok(())
}

/// Advance the wizard's "current step" cursor. No-op when the wizard is
/// already `completed`. Validated against the 1..=7 grammar so a
/// malicious IPC caller can't put the wizard into an unreachable state.
pub fn set_current_step(step: OnboardingStep) -> Result<(), OnboardingError> {
    let conn = open()?;
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE onboarding_state
            SET current_step = ?1, updated_at = ?2
          WHERE id = 1 AND completed = 0",
        params![step.to_index(), now],
    )?;
    Ok(())
}

/// Mark a single step's completion flag. The caller is the UI; the
/// underlying contract module (applock / accounts / terraform / …)
/// has already done its own validation, so we just flip the flag.
pub fn mark_step_completed(step: OnboardingStep) -> Result<(), OnboardingError> {
    let column = match step {
        OnboardingStep::Language => "step_language_completed",
        OnboardingStep::MasterPassword => "step_password_completed",
        OnboardingStep::AwsAccount => "step_account_completed",
        OnboardingStep::Terraform => "step_terraform_completed",
        OnboardingStep::BusinessContext => "step_context_completed",
        OnboardingStep::FirstScan => "step_first_scan_completed",
        OnboardingStep::Done => {
            return Err(OnboardingError::InvalidInput("step"));
        }
    };
    let conn = open()?;
    let now = Utc::now().to_rfc3339();
    // Column name is a fixed string from the match above — NEVER user-
    // controlled. The format!() is on a static set, not on a bound value.
    let sql = format!(
        "UPDATE onboarding_state SET {column} = 1, updated_at = ?1 WHERE id = 1"
    );
    conn.execute(&sql, params![now])?;
    Ok(())
}

/// Flip the global `completed` flag. The frontend calls this after the
/// FirstScan step finishes. Subsequent launches see `completed = true`
/// and route straight to the main app.
pub fn complete() -> Result<(), OnboardingError> {
    let conn = open()?;
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE onboarding_state
            SET completed = 1,
                current_step = 7,
                completed_at = ?1,
                updated_at = ?1
          WHERE id = 1",
        params![now],
    )?;
    Ok(())
}

/// Reset the wizard for a "re-run from Settings" flow. Per Contract 14
/// §Edge Cases, the user can re-enter the wizard to add another
/// account. We DO NOT clear the language or the underlying contract
/// state (accounts, master password) — only the wizard's own
/// progression. The Settings UI is responsible for routing to the
/// specific step the user picked (typically AwsAccount).
pub fn reset_for_rerun(start_at: OnboardingStep) -> Result<(), OnboardingError> {
    let conn = open()?;
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE onboarding_state
            SET completed = 0,
                current_step = ?1,
                step_account_completed = 0,
                step_terraform_completed = 0,
                step_first_scan_completed = 0,
                updated_at = ?2
          WHERE id = 1",
        params![start_at.to_index(), now],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_round_trips_through_index() {
        for s in [
            OnboardingStep::Language,
            OnboardingStep::MasterPassword,
            OnboardingStep::AwsAccount,
            OnboardingStep::Terraform,
            OnboardingStep::BusinessContext,
            OnboardingStep::FirstScan,
            OnboardingStep::Done,
        ] {
            assert_eq!(OnboardingStep::from_index(s.to_index()), s);
        }
    }

    #[test]
    fn step_index_out_of_range_falls_back_to_done() {
        assert_eq!(OnboardingStep::from_index(0), OnboardingStep::Done);
        assert_eq!(OnboardingStep::from_index(99), OnboardingStep::Done);
    }

    #[test]
    fn done_is_not_a_completable_step() {
        // mark_step_completed against Done is rejected to keep the
        // step column set strictly bounded.
        let err = mark_step_completed(OnboardingStep::Done).unwrap_err();
        assert!(matches!(err, OnboardingError::InvalidInput("step")));
    }
}
