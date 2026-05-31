// Typed error enum for the AI Suggestion Layer. Mirrors the pattern
// every other module follows — short stable codes, no raw provider text
// crosses IPC.

use thiserror::Error;

use crate::errors::AppError;

#[derive(Debug, Error)]
pub enum AiError {
    #[error("invalid input: {0}")]
    InvalidInput(&'static str),

    /// No provider key configured. The IPC surface returns this when the
    /// caller asks for a request preview or a send without a connected
    /// key. The UI uses it to route the user to Settings.
    #[error("no provider key")]
    NoProviderKey,

    /// No provider chosen in Settings. Distinct from `NoProviderKey` so
    /// the UI can localize "pick a provider" vs. "paste a key".
    #[error("no provider configured")]
    NoProvider,

    /// Provider rejected the key (revoked, expired, mis-scoped). The
    /// UI directs the user back to Settings.
    #[error("provider key invalid or expired")]
    KeyInvalid,

    /// Provider rate-limited the request. The UI offers a retry.
    ///
    /// PR #84 — kept as a sentinel for the legacy retry path but the
    /// preferred surface for any non-2xx provider response is now
    /// `ProviderError` below, which carries the actual message the
    /// provider returned (`"Your credit balance is too low"`,
    /// `"Per-minute token quota exceeded"`, etc.) so the UI doesn't
    /// guess at the cause from the status code alone.
    #[error("rate limited")]
    RateLimited,

    /// Generic transport failure (connect refused, TLS handshake, …).
    #[error("network unreachable")]
    Network,

    /// Provider responded with an unexpected status or body.
    #[error("provider error status {0}")]
    Server(u16),

    /// PR #84 — Provider responded with a non-2xx and a parseable
    /// error body. The message is the literal text the provider gave
    /// us, lightly sanitized (length-capped, no embedded credentials
    /// because the provider's own response body shouldn't carry the
    /// key we sent). Letting the user see this surfaces the real
    /// cause — "credit balance too low" vs "tokens-per-minute limit"
    /// vs "model not available to your tier" — instead of the generic
    /// "rate limited" bucket every 429 used to collapse into.
    #[error("provider error ({status}): {message}")]
    ProviderError { status: u16, message: String },

    /// The supplied finding_id wasn't found (defense-in-depth — the
    /// frontend should never get here unless the UI is out of sync).
    #[error("finding not found")]
    FindingNotFound,

    #[error("db: {0}")]
    Db(String),

    #[error("io: {0}")]
    Io(String),
}

impl AiError {
    pub fn code(&self) -> &'static str {
        match self {
            AiError::InvalidInput(_) => "invalid_input",
            AiError::NoProviderKey => "ai_no_provider_key",
            AiError::NoProvider => "ai_no_provider",
            AiError::KeyInvalid => "ai_key_invalid",
            AiError::RateLimited => "ai_rate_limited",
            AiError::Network => "ai_network",
            AiError::Server(_) => "ai_server_error",
            AiError::ProviderError { .. } => "ai_provider_error",
            AiError::FindingNotFound => "finding_not_found",
            AiError::Db(_) => "db_error",
            AiError::Io(_) => "io_error",
        }
    }
}

impl From<rusqlite::Error> for AiError {
    fn from(e: rusqlite::Error) -> Self {
        AiError::Db(e.to_string())
    }
}

impl From<AiError> for AppError {
    fn from(e: AiError) -> Self {
        match e {
            AiError::InvalidInput(f) => AppError::InvalidInput(f.into()),
            AiError::NoProviderKey => AppError::AiNoProviderKey,
            AiError::NoProvider => AppError::AiNoProvider,
            AiError::KeyInvalid => AppError::AiKeyInvalid,
            AiError::RateLimited => AppError::AiRateLimited,
            AiError::Network => AppError::AiNetwork,
            AiError::Server(s) => AppError::AiServerError(s),
            AiError::ProviderError { status, message } => {
                AppError::AiProviderError { status, message }
            }
            AiError::FindingNotFound => AppError::FindingNotFound,
            AiError::Db(m) => AppError::Db(m),
            AiError::Io(m) => AppError::Io(m),
        }
    }
}
