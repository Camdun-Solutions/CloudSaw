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
pub mod types;

pub use error::AiError;
pub use types::{
    AiRequestPreview, AiSettings, AiSuggestion, BusinessContext, ContextFlags, EnvironmentType,
    FindingDigest, Provider, RiskTolerance, TeamSize,
};

use crate::eventlog::{self, EventInput, EventKind};

/// Read everything Settings needs in one round-trip.
pub fn get_settings() -> Result<AiSettings, AiError> {
    let provider = context::read_provider()?;
    let ctx = context::read_context()?;
    let flags = context::flag_fields(&ctx);
    let key_connected = match provider {
        Some(p) => key::has(p)?,
        None => false,
    };
    Ok(AiSettings {
        provider,
        key_connected,
        context: ctx,
        flags,
    })
}

/// Persist the user's provider selection. Clearing the provider (None)
/// puts the layer back into the dormant state.
pub fn set_provider(provider: Option<Provider>) -> Result<(), AiError> {
    context::write_provider(provider)?;
    eventlog::record_event(EventInput::new(
        EventKind::SettingsChanged,
        match provider {
            Some(p) => format!("AI provider set to {}.", p.as_str()),
            None => "AI provider cleared.".to_string(),
        },
    ));
    Ok(())
}

pub fn set_provider_key(provider: Provider, value: String) -> Result<(), AiError> {
    key::set(provider, zeroize::Zeroizing::new(value))?;
    eventlog::record_event(EventInput::new(
        EventKind::SettingsChanged,
        format!("AI provider key connected ({}).", provider.as_str()),
    ));
    Ok(())
}

pub fn clear_provider_key(provider: Provider) -> Result<(), AiError> {
    key::clear(provider)?;
    eventlog::record_event(EventInput::new(
        EventKind::SettingsChanged,
        format!("AI provider key cleared ({}).", provider.as_str()),
    ));
    Ok(())
}

pub fn has_provider_key(provider: Provider) -> Result<bool, AiError> {
    key::has(provider)
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
pub fn prepare_request(
    finding_id: &str,
) -> Result<AiRequestPreview, AiError> {
    let provider = context::read_provider()?.ok_or(AiError::NoProvider)?;
    if !key::has(provider)? {
        return Err(AiError::NoProviderKey);
    }
    builder::build_preview(provider, finding_id)
}

/// Send the previously-built preview. Records an event-log row noting
/// that a request occurred (NOT the content — Contract 13 §Constraints).
pub fn send_request(preview: AiRequestPreview) -> Result<AiSuggestion, AiError> {
    // Defense in depth: the IPC bridge re-checks the gate here so a
    // direct IPC caller that skipped `prepare_request` can't smuggle
    // an arbitrary payload past the "must have key" rule.
    if !key::has(preview.provider)? {
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
        assert_eq!(Provider::from_storage("anthropic"), Some(Provider::Anthropic));
        assert_eq!(Provider::from_storage("openai"), Some(Provider::Openai));
        assert_eq!(Provider::from_storage(""), None);
        assert_eq!(Provider::from_storage("garbage"), None);
    }
}
