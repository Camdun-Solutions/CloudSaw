// PAT storage. CLAUDE.md §4.3 + Contract 12 §Constraints: the PAT lives
// ONLY in the OS keychain at `cloudsaw.github_pat`. It is fetched on
// demand and held in memory for the minimum time needed.
//
// `zeroize::Zeroizing<String>` wraps every in-memory copy so we don't
// leave the token byte-pattern in a heap arena after the function
// returns.

use zeroize::Zeroizing;

use super::error::GithubError;
use crate::keychain::{self, GITHUB_PAT_ACCOUNT, GITHUB_PAT_SERVICE};

/// Look up the PAT. Returns `Ok(None)` when no token is configured —
/// the caller (the API client) translates that into `GithubError::NoToken`
/// so the public surface speaks one vocabulary.
pub fn get() -> Result<Option<Zeroizing<String>>, GithubError> {
    match keychain::get(GITHUB_PAT_SERVICE, GITHUB_PAT_ACCOUNT) {
        Ok(Some(s)) => Ok(Some(Zeroizing::new(s))),
        Ok(None) => Ok(None),
        Err(_) => Err(GithubError::Network),
    }
}

/// Persist the user-supplied PAT. The Settings UI accepts the value
/// from a password-style input and hands it here; this function NEVER
/// logs, mirrors, or otherwise persists the secret anywhere other than
/// the keychain.
pub fn set(value: Zeroizing<String>) -> Result<(), GithubError> {
    let trimmed = value.trim();
    if !looks_like_pat(trimmed) {
        return Err(GithubError::InvalidInput("github_pat"));
    }
    keychain::set(GITHUB_PAT_SERVICE, GITHUB_PAT_ACCOUNT, trimmed)
        .map_err(|_| GithubError::Network)?;
    Ok(())
}

/// Remove the PAT. Treats `NoEntry` as success — Contract 11 §Edge Cases.
pub fn clear() -> Result<(), GithubError> {
    keychain::delete(GITHUB_PAT_SERVICE, GITHUB_PAT_ACCOUNT)
        .map(|_| ())
        .map_err(|_| GithubError::Network)
}

/// Shape check. We accept either GitHub's classic `ghp_…` or the
/// fine-grained `github_pat_…` form. Length is checked loosely — the
/// official lower bound is 40 characters, but GitHub has rotated it
/// historically so we settle for "at least 20" plus a recognized prefix.
pub fn looks_like_pat(s: &str) -> bool {
    if s.len() < 20 {
        return false;
    }
    const PREFIXES: &[&str] = &["ghp_", "ghs_", "gho_", "ghu_", "ghr_", "github_pat_"];
    PREFIXES.iter().any(|p| s.starts_with(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pat_shape_check_accepts_known_prefixes() {
        assert!(looks_like_pat("ghp_aaaaaaaaaaaaaaaaaaaaa"));
        assert!(looks_like_pat("github_pat_aaaaaaaaaaaaaaaaaaaa"));
    }

    #[test]
    fn pat_shape_check_rejects_random_or_short_strings() {
        assert!(!looks_like_pat("short"));
        assert!(!looks_like_pat("eyJhbGc.foo.bar.this.is.a.jwt.shape"));
        assert!(!looks_like_pat("Bearer some-other-token-value"));
    }
}
