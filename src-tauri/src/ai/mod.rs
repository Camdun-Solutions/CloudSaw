// AI Suggestion Layer — Contract 13.
//
// Fully OPT-IN: the layer is DORMANT until the user connects their own
// provider API key. With no key configured, no AI code path makes any
// network call. The provider relationship is entirely the user's —
// CloudSaw supplies no provider account.
//
// Every call follows the same shape:
//
//   1. `prepare_request(provider, finding_id)` builds the EXACT
//      payload that would be transmitted. The body is built by
//      construction from the finding TYPE (rule key + service +
//      severity + category) plus the structured business context —
//      never from raw ARNs, bucket names, account IDs, or any
//      user-chosen identifier.
//   2. The UI MUST display the resulting `AiRequestPreview` to the
//      user. Submission proceeds only on explicit user action.
//   3. `send_request(preview)` re-uses the same preview the user
//      reviewed — there is no last-mile rewriting in the client.
//
// Where a placeholder is needed (e.g. "block public access on
// [REDACTED-BUCKET-NAME]"), the builder uses a CONSTANT placeholder
// string. No real-value↔placeholder map exists; placeholders the
// model echoes stay as placeholders in the response. CLAUDE.md §5
// hard-DO-NOT.

pub mod builder;
pub mod client;
pub mod context;
pub mod error;
pub mod key;
pub mod providers;
pub mod types;

pub use error::AiError;
pub use providers::ProviderRecord;
pub use types::{
    AiRequestPreview, AiSettings, AiSuggestion, BusinessContext, ContextFlags, EnvironmentType,
    FindingDigest, Provider, RiskTolerance, TeamSize,
};

use crate::eventlog::{self, EventInput, EventKind};
use zeroize::Zeroizing;

/// Read everything Settings needs in one round-trip.
///
/// PR #74 — multi-provider model. `provider` + `key_connected`
/// describe the currently-active provider. The frontend reads the
/// full connected list via `list_providers()`; this helper exists for
/// the request-preview gate and the dashboard's "AI is configured"
/// badge.
pub fn get_settings() -> Result<AiSettings, AiError> {
    let active = providers::active()?;
    let ctx = context::read_context()?;
    let flags = context::flag_fields(&ctx);
    let (provider, key_connected) = match &active {
        Some(rec) => (Some(rec.provider_type), key::has_for_id(&rec.provider_id)?),
        None => (None, false),
    };
    Ok(AiSettings {
        provider,
        key_connected,
        context: ctx,
        flags,
    })
}

/// PR #74 — list every connected provider. Used by the Settings
/// "Connected Providers" list and the AI-target picker.
pub fn list_providers() -> Result<Vec<ProviderRecord>, AiError> {
    providers::list()
}

pub fn add_provider(
    provider_type: Provider,
    nickname: String,
    key: String,
) -> Result<ProviderRecord, AiError> {
    let rec = providers::add(provider_type, nickname.clone(), Zeroizing::new(key))?;
    eventlog::record_event(EventInput::new(
        EventKind::SettingsChanged,
        format!(
            "AI provider connected ({}, nickname \"{}\").",
            rec.provider_type.as_str(),
            rec.nickname
        ),
    ));
    Ok(rec)
}

pub fn update_provider(
    provider_id: String,
    nickname: Option<String>,
    new_key: Option<String>,
) -> Result<ProviderRecord, AiError> {
    let rotated_key = new_key.is_some();
    let rec = providers::update(&provider_id, nickname.clone(), new_key.map(Zeroizing::new))?;
    let summary = match (nickname.as_deref(), rotated_key) {
        (Some(_), true) => format!(
            "AI provider updated (nickname + key rotated, {}).",
            rec.provider_type.as_str()
        ),
        (Some(_), false) => format!(
            "AI provider nickname updated ({}).",
            rec.provider_type.as_str()
        ),
        (None, true) => format!("AI provider key rotated ({}).", rec.provider_type.as_str()),
        (None, false) => format!("AI provider unchanged ({}).", rec.provider_type.as_str()),
    };
    eventlog::record_event(EventInput::new(EventKind::SettingsChanged, summary));
    Ok(rec)
}

pub fn delete_provider(provider_id: String) -> Result<(), AiError> {
    let existing = providers::get(&provider_id)?.ok_or(AiError::NoProvider)?;
    providers::delete(&provider_id)?;
    eventlog::record_event(EventInput::new(
        EventKind::SettingsChanged,
        format!(
            "AI provider removed ({}, nickname \"{}\").",
            existing.provider_type.as_str(),
            existing.nickname
        ),
    ));
    Ok(())
}

pub fn set_active_provider(provider_id: String) -> Result<(), AiError> {
    providers::set_active(&provider_id)?;
    if provider_id.is_empty() {
        eventlog::record_event(EventInput::new(
            EventKind::SettingsChanged,
            "AI active provider cleared.",
        ));
    } else if let Some(rec) = providers::get(&provider_id)? {
        eventlog::record_event(EventInput::new(
            EventKind::SettingsChanged,
            format!(
                "AI active provider set to \"{}\" ({}).",
                rec.nickname,
                rec.provider_type.as_str()
            ),
        ));
    }
    Ok(())
}

/// Legacy single-provider shim. PR #74 supersedes this with the
/// multi-provider model — `set_active_provider(provider_id)`. Kept so
/// integration tests and the (now-replaced) single-provider settings
/// page keep compiling; routes through `providers::set_active` by
/// looking up the first row of the requested type, or clears the
/// active selection when given `None`.
#[deprecated(note = "Use set_active_provider(provider_id) — see PR #74 multi-provider model")]
pub fn set_provider(provider: Option<Provider>) -> Result<(), AiError> {
    match provider {
        None => set_active_provider(String::new()),
        Some(p) => {
            let found = providers::list()?
                .into_iter()
                .find(|r| r.provider_type == p)
                .ok_or(AiError::NoProvider)?;
            set_active_provider(found.provider_id)
        }
    }
}

/// Legacy single-provider shim. Adds a new row with the default
/// nickname (the provider's display name) so the legacy
/// "set the key for provider X" call still produces a working
/// Connected Provider. PR #74 supersedes with `add_provider`.
#[deprecated(note = "Use add_provider(provider_type, nickname, key) — see PR #74")]
pub fn set_provider_key(provider: Provider, value: String) -> Result<(), AiError> {
    let default_nick = match provider {
        Provider::Anthropic => "Anthropic".to_string(),
        Provider::Openai => "OpenAI".to_string(),
    };
    add_provider(provider, default_nick, value).map(|_| ())
}

/// Legacy single-provider shim. Deletes any rows matching the given
/// type. PR #74 supersedes with `delete_provider(provider_id)`.
#[deprecated(note = "Use delete_provider(provider_id) — see PR #74")]
pub fn clear_provider_key(provider: Provider) -> Result<(), AiError> {
    let matching: Vec<String> = providers::list()?
        .into_iter()
        .filter(|r| r.provider_type == provider)
        .map(|r| r.provider_id)
        .collect();
    for id in matching {
        providers::delete(&id)?;
    }
    eventlog::record_event(EventInput::new(
        EventKind::SettingsChanged,
        format!("AI provider key cleared ({}).", provider.as_str()),
    ));
    Ok(())
}

#[deprecated(note = "Use list_providers().iter().any(|r| r.provider_type == p) — see PR #74")]
pub fn has_provider_key(provider: Provider) -> Result<bool, AiError> {
    Ok(providers::list()?
        .iter()
        .any(|r| r.provider_type == provider))
}

/// Read the business context that the request builder reads from.
pub fn get_business_context() -> Result<(BusinessContext, ContextFlags), AiError> {
    let ctx = context::read_context()?;
    let flags = context::flag_fields(&ctx);
    Ok((ctx, flags))
}

pub fn set_business_context(ctx: BusinessContext) -> Result<(), AiError> {
    context::write_context(&ctx)?;
    eventlog::record_event(EventInput::new(
        EventKind::SettingsChanged,
        "AI business context updated.",
    ));
    Ok(())
}

/// Build the preview the UI MUST show before any send. The returned
/// value is the EXACT payload that would be transmitted.
///
/// PR #74 — resolves the currently-active provider (and its
/// `provider_id`) so the request preview the user reviews names the
/// exact connected-provider row whose key will sign the call.
pub fn prepare_request(finding_id: &str) -> Result<AiRequestPreview, AiError> {
    let active = providers::active()?.ok_or(AiError::NoProvider)?;
    if !key::has_for_id(&active.provider_id)? {
        return Err(AiError::NoProviderKey);
    }
    builder::build_preview(active.provider_type, &active.provider_id, finding_id)
}

/// Send the previously-built preview. Records an event-log row noting
/// that a request occurred (NOT the content — Contract 13 §Constraints).
pub fn send_request(preview: AiRequestPreview) -> Result<AiSuggestion, AiError> {
    // Defense in depth: the IPC bridge re-checks the gate here so a
    // direct IPC caller that skipped `prepare_request` can't smuggle
    // an arbitrary payload past the "must have key" rule. The check
    // hits the new per-id slot when populated, falls back to the
    // legacy type-keyed slot otherwise.
    let key_present = if preview.provider_id.is_empty() {
        key::has(preview.provider)?
    } else {
        key::has_for_id(&preview.provider_id)?
    };
    if !key_present {
        return Err(AiError::NoProviderKey);
    }
    let suggestion = client::send_with_provider_key(&preview)?;
    eventlog::record_event(EventInput::new(
        EventKind::SettingsChanged,
        format!(
            "AI suggestion received from {} (model {}).",
            preview.provider.as_str(),
            preview.model
        ),
    ));
    Ok(suggestion)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_round_trips_through_storage_form() {
        assert_eq!(Provider::Anthropic.as_str(), "anthropic");
        assert_eq!(Provider::Openai.as_str(), "openai");
        assert_eq!(
            Provider::from_storage("anthropic"),
            Some(Provider::Anthropic)
        );
        assert_eq!(Provider::from_storage("openai"), Some(Provider::Openai));
        assert_eq!(Provider::from_storage(""), None);
        assert_eq!(Provider::from_storage("garbage"), None);
    }
}
