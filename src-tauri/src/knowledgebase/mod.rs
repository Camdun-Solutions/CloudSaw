// Knowledge base & compliance control mapping — Contract 08.
//
// Bundled markdown articles map each ScoutSuite finding type to remediation
// guidance, and a separate dataset maps the same finding types to controls
// across SOC 2, ISO 27001, HIPAA, and NIST. Both ship with the binary so
// the app works fully offline (CLAUDE.md §1); an opt-in remote refresh
// (default OFF) can replace the bundled set with newer content from a
// public documentation repo.
//
// Public surface (mirrors Contract 08 §Expected Output):
//
//     get_article(finding_id)         -> KnowledgeArticle
//     list_articles()                 -> Vec<ArticleSummary>
//     get_control_mappings(finding_id)-> ControlMapping
//     list_frameworks()               -> Vec<Framework>
//     check_for_kb_update()           -> RefreshCheckResult
//     apply_kb_update()               -> RefreshApplyResult
//     get_refresh_settings()          -> RefreshSettings
//     set_refresh_settings(update)    -> RefreshSettings
//
// What this module DOES NOT do:
//   - Render markdown to HTML. The raw string crosses IPC and the
//     frontend renders + sanitizes (Contract 08 §Constraints, Contract 09).
//   - Transmit account or finding data anywhere. The remote refresh fetches
//     public documentation only; no headers, query parameters, or bodies
//     carry account information (CLAUDE.md §5 hard-DO-NOT).
//   - Persist credentials. No credential-bearing values cross this module
//     at any point.

pub mod bundled;
pub mod error;
pub mod parser;
pub mod refresh;
pub mod registry;
pub mod scoutsuite;
pub mod storage;
pub mod types;

pub use error::KnowledgebaseError;
pub use types::{
    ArticleSummary, ControlMapping, ControlReference, Framework, KnowledgeArticle, KnowledgeSource,
    RefreshApplyResult, RefreshCheckResult, RefreshSettings, RefreshSettingsUpdate,
};

use crate::errors::AppError;

/// Bootstrap the knowledgebase content cache. Called once during app
/// startup, after migrations run, so the first `get_article` call doesn't
/// pay the markdown-loading cost on the UI thread.
pub fn bootstrap() -> Result<(), AppError> {
    refresh::bootstrap_active_content().map_err(AppError::from)
}

/// Article lookup by finding ID. A finding without a bundled article
/// returns a default article with `matched = false` — never an error.
pub fn get_article(finding_id: &str) -> Result<KnowledgeArticle, KnowledgebaseError> {
    validate_finding_id(finding_id)?;
    registry::get_article(finding_id)
}

/// Every article currently in the active set (bundled or remote), sorted
/// by finding_id.
pub fn list_articles() -> Result<Vec<ArticleSummary>, KnowledgebaseError> {
    registry::list_articles()
}

/// Compliance control mapping for a finding. A finding with no mappings
/// returns an empty frameworks map — never an error.
pub fn get_control_mappings(finding_id: &str) -> Result<ControlMapping, KnowledgebaseError> {
    validate_finding_id(finding_id)?;
    registry::get_control_mappings(finding_id)
}

/// Supported frameworks. Adding a new framework's data to the bundled
/// mappings.json (or to a remote bundle) makes it appear here with no
/// code change (Contract 08 §Edge Cases).
pub fn list_frameworks() -> Result<Vec<Framework>, KnowledgebaseError> {
    registry::list_frameworks()
}

/// Non-mutating probe of the remote source. Fails with
/// `RefreshDisabled` if the user has not opted in.
pub fn check_for_kb_update() -> Result<RefreshCheckResult, KnowledgebaseError> {
    let url = storage::get_repo_url()?;
    let fetcher = refresh::default_fetcher_for(&url);
    refresh::check_for_kb_update(fetcher.as_ref())
}

/// Apply the latest remote bundle. On failure, the bundled baseline (or
/// any prior remote cache) remains untouched.
pub fn apply_kb_update() -> Result<RefreshApplyResult, KnowledgebaseError> {
    let url = storage::get_repo_url()?;
    let fetcher = refresh::default_fetcher_for(&url);
    refresh::apply_kb_update(fetcher.as_ref())
}

/// Current refresh settings. Always returns a value — defaults populate
/// every field if no row exists yet.
pub fn get_refresh_settings() -> Result<RefreshSettings, KnowledgebaseError> {
    Ok(RefreshSettings {
        enabled: storage::is_refresh_enabled()?,
        repo_url: storage::get_repo_url()?,
        remote_active: storage::is_remote_active()?,
        last_checked_at: storage::get_last_checked_at()?,
        last_applied_at: storage::get_last_applied_at()?,
        last_error: storage::get_last_error()?,
    })
}

/// Apply a partial settings update. Disabling the feature drops the
/// on-disk cache and reverts to the bundled baseline (Contract 08
/// §Security Check + §Constraints).
pub fn set_refresh_settings(
    update: RefreshSettingsUpdate,
) -> Result<RefreshSettings, KnowledgebaseError> {
    if let Some(url) = update.repo_url.as_deref() {
        storage::set_repo_url(url)?;
    }
    if let Some(enabled) = update.enabled {
        let was_enabled = storage::is_refresh_enabled()?;
        if was_enabled && !enabled {
            refresh::disable_and_revert()?;
        } else {
            storage::set_refresh_enabled(enabled)?;
        }
    }
    get_refresh_settings()
}

/// Test seam: invoke `apply_kb_update` against a caller-supplied fetcher.
/// Production callers go through the URL-derived default.
#[doc(hidden)]
pub fn apply_kb_update_with_fetcher(
    fetcher: &dyn refresh::Fetcher,
) -> Result<RefreshApplyResult, KnowledgebaseError> {
    refresh::apply_kb_update(fetcher)
}

/// Test seam companion: same idea, for `check_for_kb_update`.
#[doc(hidden)]
pub fn check_for_kb_update_with_fetcher(
    fetcher: &dyn refresh::Fetcher,
) -> Result<RefreshCheckResult, KnowledgebaseError> {
    refresh::check_for_kb_update(fetcher)
}

fn validate_finding_id(id: &str) -> Result<(), KnowledgebaseError> {
    if id.is_empty() || id.len() > 128 {
        return Err(KnowledgebaseError::InvalidInput("finding_id"));
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    {
        return Err(KnowledgebaseError::InvalidInput("finding_id"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_finding_id_accepts_scoutsuite_grammar() {
        assert!(validate_finding_id("iam-user-no-mfa").is_ok());
        assert!(validate_finding_id("vpc_flow_logs_disabled").is_ok());
        assert!(validate_finding_id("").is_err());
        assert!(validate_finding_id("iam/path").is_err());
        assert!(validate_finding_id("IAM-Upper").is_err());
    }
}
