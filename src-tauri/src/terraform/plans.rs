// In-memory plan-token store.
//
// Contract 05 §Constraints:
//   * "`apply` MUST run only after the user confirms a plan diff; `plan_token`
//     ties an apply to a specific plan output."
//   * "The user runs `apply` with a stale `plan_token` (a newer plan exists)
//     → the stale apply is rejected."
//
// Tokens are ephemeral by design — they live as long as the running process,
// and on restart the user must re-plan before applying. That's a feature, not
// a bug: a stale plan after an app restart should not be silently applied.
// Persisting tokens to SQLite would add complexity and a stale-state risk
// with no real upside, so we keep this in-memory.
//
// Concurrency model: a `Mutex<HashMap<String, PlanEntry>>` keyed on the AWS
// account ID. There is at most ONE active plan per account; minting a new
// plan supersedes the prior one, and `consume` removes the entry when apply
// succeeds (or when apply fails — re-apply must re-plan, so removing on
// failure too is intentional).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use chrono::{DateTime, Utc};
use rand_core::{OsRng, RngCore};

use super::error::TerraformError;
use super::types::{PlanChange, PolicyVariant};

/// What we remember about an outstanding plan so `apply` can verify it. The
/// `plan_file` path lets `terraform apply` target the exact binary plan the
/// user confirmed.
#[derive(Debug, Clone)]
pub struct PlanEntry {
    pub plan_token: String,
    pub aws_account_id: String,
    pub plan_file: PathBuf,
    pub planned_principal_arn: String,
    pub policy_variant: PolicyVariant,
    pub no_changes: bool,
    pub changes: Vec<PlanChange>,
    pub created_at: DateTime<Utc>,
}

fn store() -> &'static Mutex<HashMap<String, PlanEntry>> {
    static S: OnceLock<Mutex<HashMap<String, PlanEntry>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Generate a fresh, opaque token. 128 random bits, hex-encoded — collision
/// chance is negligible and the value tells the UI nothing about state.
pub fn mint_token() -> String {
    let mut bytes = [0u8; 16];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Install a new plan entry, evicting any prior entry for the same account.
/// A subsequent `apply` against a stale (evicted) token will surface
/// `PlanTokenExpired`.
pub fn insert(entry: PlanEntry) {
    let mut guard = store().lock().unwrap_or_else(|p| p.into_inner());
    guard.insert(entry.aws_account_id.clone(), entry);
}

/// Look up the active plan for `aws_account_id`. Returns `None` if the
/// account has no outstanding plan.
pub fn peek(aws_account_id: &str) -> Option<PlanEntry> {
    let guard = store().lock().unwrap_or_else(|p| p.into_inner());
    guard.get(aws_account_id).cloned()
}

/// Consume the plan if its token matches `plan_token`. The match policy is:
///   * `Ok(entry)` — token matches the current entry; entry is removed.
///   * `Err(PlanTokenExpired)` — there IS an entry, but its token is
///     different (a newer plan has superseded the one the UI is holding).
///   * `Err(PlanTokenInvalid)` — no entry at all.
pub fn consume(aws_account_id: &str, plan_token: &str) -> Result<PlanEntry, TerraformError> {
    let mut guard = store().lock().unwrap_or_else(|p| p.into_inner());
    let entry = match guard.get(aws_account_id) {
        Some(e) => e.clone(),
        None => return Err(TerraformError::PlanTokenInvalid),
    };
    if entry.plan_token != plan_token {
        return Err(TerraformError::PlanTokenExpired);
    }
    guard.remove(aws_account_id);
    Ok(entry)
}

/// Test seam: clear all outstanding tokens. Used by integration tests that
/// share the process-global store.
#[cfg(any(test, debug_assertions))]
pub fn _clear_for_tests() {
    let mut guard = store().lock().unwrap_or_else(|p| p.into_inner());
    guard.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_entry(account: &str, token: &str) -> PlanEntry {
        PlanEntry {
            plan_token: token.to_string(),
            aws_account_id: account.to_string(),
            plan_file: PathBuf::from("/tmp/plan"),
            planned_principal_arn: "arn:aws:iam::111122223333:role/x".into(),
            policy_variant: PolicyVariant::SecurityAudit,
            no_changes: false,
            changes: Vec::new(),
            created_at: Utc::now(),
        }
    }

    #[test]
    fn mint_token_yields_unique_hex_strings() {
        let a = mint_token();
        let b = mint_token();
        assert_ne!(a, b);
        for t in [&a, &b] {
            assert_eq!(t.len(), 32);
            assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
        }
    }

    // These tests share a process-global Mutex<HashMap> store. Each test
    // uses a distinct account ID so they cannot collide; we avoid calling
    // `_clear_for_tests` here because the cargo test harness runs tests in
    // parallel and a global clear from one test would race a peek in another.

    #[test]
    fn consume_matches_token_and_evicts_entry() {
        let account = "999911110001";
        let token = "consume-match-token";
        insert(fake_entry(account, token));
        let entry = consume(account, token).unwrap();
        assert_eq!(entry.plan_token, token);
        // Second consume of the same token must fail — entry was evicted.
        assert!(matches!(
            consume(account, token),
            Err(TerraformError::PlanTokenInvalid)
        ));
    }

    #[test]
    fn consume_with_stale_token_returns_expired() {
        let account = "999911110002";
        insert(fake_entry(account, "fresh"));
        // A second plan supersedes the first:
        insert(fake_entry(account, "fresh-2"));
        assert!(matches!(
            consume(account, "fresh"),
            Err(TerraformError::PlanTokenExpired)
        ));
        // The newer token is still applyable.
        assert!(consume(account, "fresh-2").is_ok());
    }

    #[test]
    fn consume_with_no_entry_returns_invalid() {
        // Use an account ID no other test inserts under so we don't depend
        // on test ordering.
        let account = "999911110003";
        assert!(matches!(
            consume(account, "anything"),
            Err(TerraformError::PlanTokenInvalid)
        ));
    }
}
