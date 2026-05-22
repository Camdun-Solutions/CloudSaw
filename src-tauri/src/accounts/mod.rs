// Multi-account configuration — Contract 04.
//
// CloudSaw users operate multiple AWS accounts (typically dev and prod). This
// module owns the `accounts` SQLite table and the active-account selection.
//
// Public surface (mirrors Contract 04 §Expected Output):
//
//     add_account(input)              -> Account
//     update_account(input)           -> Account
//     remove_account(aws_account_id)  -> RemovalImpact
//     list_accounts()                 -> Vec<Account>
//     get_account(aws_account_id)     -> Account
//     set_active_account(opt id)      -> ()
//     get_active_account()            -> Option<String>
//     get_display_settings()          -> AccountsDisplaySettings
//     set_display_settings(s)         -> ()
//
// Things this module DOES NOT do (and never will):
//   - Store credentials. The table is configuration only. Credentials are
//     resolved by the AWS SDK provider chain at scan time (CLAUDE.md §4.3).
//   - Accept a UI-supplied AWS account ID. Every stored ID is the result of
//     `sts:GetCallerIdentity` (Contract 04 §Constraints).
//   - Hold an active "session". Active account is a stored singleton; there
//     is no global mutable state here.

pub mod error;
pub mod storage;
pub mod types;
mod validation;

pub use error::AccountsError;
pub use types::{
    Account, AccountsDisplaySettings, AddAccountInput, Environment, RemovalImpact, ScanOutcome,
    UpdateAccountInput,
};

use crate::auth;

/// Verify the profile via STS, then persist a new accounts row. The verified
/// account ID — never a UI-supplied value — becomes the row's primary key.
///
/// Failure modes:
///   * `Verification(_)` — STS rejected the profile (SSO expired, no
///     permission, timeout, …). The row is not written.
///   * `DuplicateAwsAccountId` — the verified ID matches an existing row.
///   * `DuplicateLabel` — the requested label is already taken.
///   * `InvalidInput(field)` — label/profile/environment failed validation.
pub async fn add_account(input: AddAccountInput) -> Result<Account, AccountsError> {
    validation::validate_label(&input.label)?;
    validation::validate_profile_name(&input.profile_name)?;
    validation::validate_environment(input.environment.as_str())?;

    let identity = auth::get_caller_identity(&input.profile_name).await?;
    validation::validate_aws_account_id(&identity.account_id)?;

    let inserted = storage::insert(&types::AccountRecord {
        aws_account_id: identity.account_id,
        label: input.label,
        profile_name: input.profile_name,
        environment: input.environment,
    })?;

    promote_if_no_active(&inserted.aws_account_id)?;
    crate::eventlog::record_event(
        crate::eventlog::EventInput::new(
            crate::eventlog::EventKind::AccountAdded,
            format!("Account \"{}\" added.", inserted.label),
        )
        .with_account(inserted.aws_account_id.clone()),
    );
    Ok(inserted)
}

/// QA state transition "Zero accounts → one account added → active account
/// set." When the user adds an account with no active selection, there is
/// exactly one valid choice — promote it automatically rather than asking
/// the UI to make a follow-up call. Subsequent adds leave the existing
/// active selection alone (the user picks deliberately via `set_active_account`).
///
/// Extracted into a helper so the storage-only integration tests can
/// exercise the rule without going through STS.
pub fn promote_if_no_active(candidate: &str) -> Result<(), AccountsError> {
    if storage::get_active()?.is_none() {
        storage::set_active(Some(candidate))?;
    }
    Ok(())
}

/// Update mutable fields. Changing `profile_name` re-runs STS verification:
/// if the new profile resolves to a DIFFERENT AWS account ID, we raise
/// `AwsAccountIdMismatch` rather than silently re-pointing the row at a new
/// identity — that would be a data-corruption bug for every account-scoped
/// table downstream.
pub async fn update_account(input: UpdateAccountInput) -> Result<Account, AccountsError> {
    validation::validate_label(&input.label)?;
    validation::validate_profile_name(&input.profile_name)?;
    validation::validate_environment(input.environment.as_str())?;
    validation::validate_aws_account_id(&input.aws_account_id)?;

    let existing = storage::get(&input.aws_account_id)?;

    if existing.profile_name != input.profile_name {
        let identity = auth::get_caller_identity(&input.profile_name).await?;
        if identity.account_id != input.aws_account_id {
            return Err(AccountsError::AwsAccountIdMismatch);
        }
    }

    storage::update_fields(
        &input.aws_account_id,
        &input.label,
        &input.profile_name,
        input.environment,
    )
}

/// Remove an account row. If the row was the active account, the active
/// selection is cleared in the same transaction. Also cascades to the
/// scheduler so a removed account doesn't leave an orphan schedule that
/// would fire against a vanished AWS context.
///
/// Right now the returned `RemovalImpact` only reports `was_active` — later
/// contracts that add scan/findings/tf-work tables will populate the counts
/// here without changing this signature.
pub fn remove_account(aws_account_id: &str) -> Result<RemovalImpact, AccountsError> {
    validation::validate_aws_account_id(aws_account_id)?;
    let was_active = storage::delete(aws_account_id)?;
    // Best-effort schedule cascade. A residual schedule for a deleted
    // account would still be skipped by the runner's gate-check, but
    // removing it keeps the Settings UI list accurate and avoids
    // surprising the user with a stale row.
    let _ = crate::scheduler::clear_schedule_if_present(aws_account_id);
    crate::eventlog::record_event(
        crate::eventlog::EventInput::new(
            crate::eventlog::EventKind::AccountRemoved,
            "Account removed.",
        )
        .with_account(aws_account_id.to_string()),
    );
    Ok(RemovalImpact {
        scans: 0,
        findings: 0,
        tf_work: 0,
        was_active,
    })
}

pub fn list_accounts() -> Result<Vec<Account>, AccountsError> {
    storage::list()
}

pub fn get_account(aws_account_id: &str) -> Result<Account, AccountsError> {
    validation::validate_aws_account_id(aws_account_id)?;
    storage::get(aws_account_id)
}

/// Set or clear the active account. `None` clears the selection (used by
/// the UI when the user wants to deselect, and internally by `remove_account`).
pub fn set_active_account(aws_account_id: Option<&str>) -> Result<(), AccountsError> {
    if let Some(id) = aws_account_id {
        validation::validate_aws_account_id(id)?;
    }
    storage::set_active(aws_account_id)
}

pub fn get_active_account() -> Result<Option<String>, AccountsError> {
    storage::get_active()
}

pub fn get_display_settings() -> Result<AccountsDisplaySettings, AccountsError> {
    Ok(AccountsDisplaySettings {
        reveal_full_ids: storage::get_reveal_full_ids()?,
    })
}

pub fn set_display_settings(settings: AccountsDisplaySettings) -> Result<(), AccountsError> {
    storage::set_reveal_full_ids(settings.reveal_full_ids)
}

/// Mask a 12-digit AWS account ID to the last 4 digits. Used for log lines
/// and as the default UI display value. Inputs shorter than 4 chars are
/// returned as a fixed-length sentinel so we never accidentally leak the
/// whole value via a length-based fallback.
pub fn mask_for_logs(aws_account_id: &str) -> String {
    let chars: Vec<char> = aws_account_id.chars().collect();
    if chars.len() < 4 {
        return "****".to_string();
    }
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("****{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_returns_last_four_digits() {
        assert_eq!(mask_for_logs("111122223333"), "****3333");
        assert_eq!(mask_for_logs("000000001234"), "****1234");
    }

    #[test]
    fn mask_handles_short_inputs_without_leaking() {
        assert_eq!(mask_for_logs(""), "****");
        assert_eq!(mask_for_logs("12"), "****");
        // 4 chars is the boundary: we still take the last 4, which equals
        // the whole string — but the masking semantic is preserved (we
        // never expose more than the trailing 4 digits).
        assert_eq!(mask_for_logs("1234"), "****1234");
    }
}
