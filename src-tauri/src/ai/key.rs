// Provider API key storage. CLAUDE.md §4.3 + Contract 13 §Constraints:
//
//   * The key lives ONLY in the OS keychain at `cloudsaw.llm_api_key`.
//     The `account` slot identifies the provider so a user can swap
//     providers without losing the other key.
//   * Fetched on demand and held in memory minimally
//     (`Zeroizing<String>` wraps every in-memory copy).
//   * The panic wipe (Contract 11) enumerates both rows.

use zeroize::Zeroizing;

use super::error::AiError;
use super::types::Provider;
use crate::keychain::{
    self, LLM_KEY_ACCOUNT_ANTHROPIC, LLM_KEY_ACCOUNT_GEMINI, LLM_KEY_ACCOUNT_OPENAI,
    LLM_KEY_SERVICE,
};

fn account_for(provider: Provider) -> &'static str {
    match provider {
        Provider::Anthropic => LLM_KEY_ACCOUNT_ANTHROPIC,
        Provider::Openai => LLM_KEY_ACCOUNT_OPENAI,
        Provider::Gemini => LLM_KEY_ACCOUNT_GEMINI,
    }
}

pub fn get(provider: Provider) -> Result<Option<Zeroizing<String>>, AiError> {
    match keychain::get(LLM_KEY_SERVICE, account_for(provider)) {
        Ok(Some(s)) => Ok(Some(Zeroizing::new(s))),
        Ok(None) => Ok(None),
        Err(_) => Err(AiError::Network),
    }
}

pub fn set(provider: Provider, value: Zeroizing<String>) -> Result<(), AiError> {
    let trimmed = value.trim();
    if !looks_like_key(provider, trimmed) {
        return Err(AiError::InvalidInput("ai_api_key"));
    }
    keychain::set(LLM_KEY_SERVICE, account_for(provider), trimmed).map_err(|_| AiError::Network)
}

pub fn clear(provider: Provider) -> Result<(), AiError> {
    keychain::delete(LLM_KEY_SERVICE, account_for(provider))
        .map(|_| ())
        .map_err(|_| AiError::Network)
}

pub fn has_any() -> Result<bool, AiError> {
    Ok(get(Provider::Anthropic)?.is_some()
        || get(Provider::Openai)?.is_some()
        || get(Provider::Gemini)?.is_some())
}

pub fn has(provider: Provider) -> Result<bool, AiError> {
    Ok(get(provider)?.is_some())
}

// PR #74 — per-provider-id keychain helpers. The multi-provider model
// (see ai::providers) stores one key per `provider_id` slug instead of
// one per provider TYPE. Legacy single-provider rows kept their
// account name (`anthropic` | `openai`) as the provider_id so the
// existing keychain entry is reusable without a transfer step.

/// Set the key for an `add()`-generated provider_id. Trim + shape-check
/// before persisting.
pub fn set_for_id(provider_id: &str, value: Zeroizing<String>) -> Result<(), AiError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AiError::InvalidInput("ai_api_key"));
    }
    keychain::set(LLM_KEY_SERVICE, provider_id, trimmed).map_err(|_| AiError::Network)
}

/// Read the key for a provider_id. Returns `None` if the keychain
/// entry was deleted out-of-band (e.g. by `keychain::wipe_all`).
pub fn get_for_id(provider_id: &str) -> Result<Option<Zeroizing<String>>, AiError> {
    match keychain::get(LLM_KEY_SERVICE, provider_id) {
        Ok(Some(s)) => Ok(Some(Zeroizing::new(s))),
        Ok(None) => Ok(None),
        Err(_) => Err(AiError::Network),
    }
}

/// Remove the keychain entry for a provider_id. Soft-errors are
/// surfaced as `AiError::Network`; "no such entry" is `Ok(())`.
pub fn clear_for_id(provider_id: &str) -> Result<(), AiError> {
    keychain::delete(LLM_KEY_SERVICE, provider_id)
        .map(|_| ())
        .map_err(|_| AiError::Network)
}

/// Whether the keychain has a key for a provider_id.
pub fn has_for_id(provider_id: &str) -> Result<bool, AiError> {
    Ok(get_for_id(provider_id)?.is_some())
}

/// Shape check per provider. We accept Anthropic's `sk-ant-…`,
/// OpenAI's `sk-…`, and Google's `AIza…` (Gemini API key format).
/// The check is intentionally lax — providers rotate prefixes, so
/// the network layer is the authority on "this key works."
pub fn looks_like_key(provider: Provider, s: &str) -> bool {
    if s.len() < 20 {
        return false;
    }
    match provider {
        Provider::Anthropic => s.starts_with("sk-ant-") || s.starts_with("sk-"),
        Provider::Openai => s.starts_with("sk-") || s.starts_with("sess-"),
        // PR #77 — Google AI Studio / Gemini keys are issued under
        // the `AIza` prefix, 39 chars total. We loosen to ≥20 to
        // match the rest of the providers' shape gates rather than
        // hard-coding the exact length, in case Google rotates the
        // format.
        Provider::Gemini => s.starts_with("AIza"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_shape_check_accepts_known_prefixes() {
        assert!(looks_like_key(Provider::Anthropic, "sk-ant-aaaaaaaaaaaaaa"));
        assert!(looks_like_key(Provider::Openai, "sk-aaaaaaaaaaaaaaaaaaaa"));
    }

    #[test]
    fn key_shape_check_rejects_short_or_unrecognized() {
        assert!(!looks_like_key(Provider::Anthropic, "short"));
        assert!(!looks_like_key(Provider::Openai, "ghp_thisisapat123456789"));
    }
}
