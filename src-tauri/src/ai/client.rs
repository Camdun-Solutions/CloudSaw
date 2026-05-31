// Provider client — Contract 13.
//
// Two providers (Anthropic, OpenAI) sit behind one `Transport` trait
// so the integration tests inject a fake. The production paths use
// `reqwest::blocking` (same pattern as `knowledgebase::refresh` and
// `github::client`), and the key travels through `Zeroizing<String>`
// end-to-end.
//
// Things this module never does:
//   * Substitute placeholders. The bytes the user reviewed in the
//     preview modal are the bytes that go to the wire.
//   * Maintain a real-value↔placeholder map. Placeholders stay as
//     placeholders in the response (Contract 13 §Constraints).
//   * Log request or response content. Event-log entries record that
//     a request occurred, not what was in it.

use std::time::Duration;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use super::error::AiError;
use super::key;
use super::types::{AiRequestPreview, AiSuggestion, Provider};

const ANTHROPIC_API: &str = "https://api.anthropic.com/v1/messages";
const OPENAI_API: &str = "https://api.openai.com/v1/chat/completions";
// PR #77 — Google Gemini v1beta `generateContent` endpoint. The
// model id is appended at request time (`/models/{model}:generate
// Content`) and the API key travels as a query string per Google's
// AI Studio convention; the HTTP body shape is documented at
// https://ai.google.dev/api/generate-content.
const GEMINI_API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta/models";

/// Transport abstraction used by tests. Production code goes through
/// `ReqwestTransport`. The transport receives the EXACT bytes built by
/// `builder::build_preview`; it does no further rewriting.
pub trait Transport: Send + Sync {
    fn send(&self, preview: &AiRequestPreview, token: &str) -> Result<AiSuggestion, AiError>;
}

pub struct ReqwestTransport;

impl Transport for ReqwestTransport {
    fn send(&self, preview: &AiRequestPreview, token: &str) -> Result<AiSuggestion, AiError> {
        match preview.provider {
            Provider::Anthropic => send_anthropic(preview, token),
            Provider::Openai => send_openai(preview, token),
            Provider::Gemini => send_gemini(preview, token),
        }
    }
}

fn build_http_client() -> Result<reqwest::blocking::Client, AiError> {
    reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .user_agent(concat!("CloudSaw/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| AiError::Network)
}

#[derive(Serialize)]
struct AnthropicMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct AnthropicRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    system: &'a str,
    messages: Vec<AnthropicMessage<'a>>,
}

#[derive(Deserialize)]
struct AnthropicContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    #[serde(default)]
    text: String,
}

#[derive(Deserialize)]
struct AnthropicUsage {
    #[serde(default)]
    input_tokens: Option<u32>,
    #[serde(default)]
    output_tokens: Option<u32>,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    model: String,
    #[serde(default)]
    content: Vec<AnthropicContentBlock>,
    #[serde(default)]
    usage: Option<AnthropicUsage>,
}

fn send_anthropic(preview: &AiRequestPreview, token: &str) -> Result<AiSuggestion, AiError> {
    let client = build_http_client()?;
    let req = AnthropicRequest {
        model: &preview.model,
        max_tokens: 1024,
        system: &preview.system_prompt,
        messages: vec![AnthropicMessage {
            role: "user",
            content: &preview.user_message,
        }],
    };
    let resp = client
        .post(ANTHROPIC_API)
        .header("x-api-key", token)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&req)
        .send()
        .map_err(|_| AiError::Network)?;

    let status = resp.status();
    if !status.is_success() {
        // PR #84 — read the body so the UI sees Anthropic's actual
        // error message ("Number of request tokens has exceeded …",
        // "Your credit balance is too low", "model: not_found_error",
        // etc.) instead of the generic status-to-bucket mapping.
        let body = resp.bytes().map(|b| b.to_vec()).unwrap_or_default();
        return Err(map_response_error(status.as_u16(), body));
    }
    let body: AnthropicResponse = resp.json().map_err(|_| AiError::Server(status.as_u16()))?;
    let text = body
        .content
        .into_iter()
        .filter(|c| c.block_type == "text")
        .map(|c| c.text)
        .collect::<Vec<_>>()
        .join("\n\n");
    let (input_tokens, output_tokens) = body
        .usage
        .map(|u| (u.input_tokens, u.output_tokens))
        .unwrap_or((None, None));
    Ok(AiSuggestion {
        provider: Provider::Anthropic,
        model: body.model,
        generated_at: Utc::now(),
        suggestion_markdown: text,
        usage_input_tokens: input_tokens,
        usage_output_tokens: output_tokens,
    })
}

#[derive(Serialize)]
struct OpenaiMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct OpenaiRequest<'a> {
    model: &'a str,
    messages: Vec<OpenaiMessage<'a>>,
    max_tokens: u32,
}

#[derive(Deserialize)]
struct OpenaiChoiceMessage {
    #[serde(default)]
    content: String,
}

#[derive(Deserialize)]
struct OpenaiChoice {
    message: OpenaiChoiceMessage,
}

#[derive(Deserialize)]
struct OpenaiUsage {
    #[serde(default)]
    prompt_tokens: Option<u32>,
    #[serde(default)]
    completion_tokens: Option<u32>,
}

#[derive(Deserialize)]
struct OpenaiResponse {
    model: String,
    #[serde(default)]
    choices: Vec<OpenaiChoice>,
    #[serde(default)]
    usage: Option<OpenaiUsage>,
}

fn send_openai(preview: &AiRequestPreview, token: &str) -> Result<AiSuggestion, AiError> {
    let client = build_http_client()?;
    let req = OpenaiRequest {
        model: &preview.model,
        max_tokens: 1024,
        messages: vec![
            OpenaiMessage {
                role: "system",
                content: &preview.system_prompt,
            },
            OpenaiMessage {
                role: "user",
                content: &preview.user_message,
            },
        ],
    };
    let resp = client
        .post(OPENAI_API)
        .bearer_auth(token)
        .header("content-type", "application/json")
        .json(&req)
        .send()
        .map_err(|_| AiError::Network)?;

    let status = resp.status();
    if !status.is_success() {
        // PR #84 — read OpenAI's error body and surface the real
        // `error.message` so the UI shows e.g. "You exceeded your
        // current quota" instead of "rate limited".
        let body = resp.bytes().map(|b| b.to_vec()).unwrap_or_default();
        return Err(map_response_error(status.as_u16(), body));
    }
    let body: OpenaiResponse = resp.json().map_err(|_| AiError::Server(status.as_u16()))?;
    let text = body
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content)
        .unwrap_or_default();
    let (input_tokens, output_tokens) = body
        .usage
        .map(|u| (u.prompt_tokens, u.completion_tokens))
        .unwrap_or((None, None));
    Ok(AiSuggestion {
        provider: Provider::Openai,
        model: body.model,
        generated_at: Utc::now(),
        suggestion_markdown: text,
        usage_input_tokens: input_tokens,
        usage_output_tokens: output_tokens,
    })
}

// --- Google Gemini transport (PR #77) -----------------------------------
//
// The Gemini API splits its request body across "contents" (the
// conversation; CloudSaw sends a single user turn) and a
// "systemInstruction" carrying the same constant system prompt
// Anthropic and OpenAI receive. Token usage shows up under
// `usageMetadata` (`promptTokenCount` / `candidatesTokenCount`).

#[derive(Serialize)]
struct GeminiTextPart<'a> {
    text: &'a str,
}

#[derive(Serialize)]
struct GeminiContent<'a> {
    role: &'a str,
    parts: Vec<GeminiTextPart<'a>>,
}

#[derive(Serialize)]
struct GeminiGenerationConfig {
    #[serde(rename = "maxOutputTokens")]
    max_output_tokens: u32,
}

#[derive(Serialize)]
struct GeminiRequest<'a> {
    #[serde(rename = "systemInstruction")]
    system_instruction: GeminiContent<'a>,
    contents: Vec<GeminiContent<'a>>,
    #[serde(rename = "generationConfig")]
    generation_config: GeminiGenerationConfig,
}

#[derive(Deserialize)]
struct GeminiPart {
    #[serde(default)]
    text: String,
}

#[derive(Deserialize)]
struct GeminiContentResp {
    #[serde(default)]
    parts: Vec<GeminiPart>,
}

#[derive(Deserialize)]
struct GeminiCandidate {
    #[serde(default)]
    content: Option<GeminiContentResp>,
}

#[derive(Deserialize)]
struct GeminiUsageMetadata {
    #[serde(default)]
    #[serde(rename = "promptTokenCount")]
    prompt_token_count: Option<u32>,
    #[serde(default)]
    #[serde(rename = "candidatesTokenCount")]
    candidates_token_count: Option<u32>,
}

#[derive(Deserialize)]
struct GeminiResponse {
    #[serde(default)]
    candidates: Vec<GeminiCandidate>,
    #[serde(default)]
    #[serde(rename = "usageMetadata")]
    usage_metadata: Option<GeminiUsageMetadata>,
}

fn send_gemini(preview: &AiRequestPreview, token: &str) -> Result<AiSuggestion, AiError> {
    let client = build_http_client()?;
    let req = GeminiRequest {
        system_instruction: GeminiContent {
            role: "system",
            parts: vec![GeminiTextPart {
                text: &preview.system_prompt,
            }],
        },
        contents: vec![GeminiContent {
            role: "user",
            parts: vec![GeminiTextPart {
                text: &preview.user_message,
            }],
        }],
        generation_config: GeminiGenerationConfig {
            max_output_tokens: 1024,
        },
    };
    // Gemini's auth: API key as the `x-goog-api-key` header. AI
    // Studio examples often show the key in the URL query string;
    // the header form is equally supported by the v1beta endpoint
    // and keeps the credential out of any URL log (defense in depth
    // even though reqwest doesn't log URLs by default).
    let url = format!(
        "{base}/{model}:generateContent",
        base = GEMINI_API_BASE,
        model = preview.model,
    );
    let resp = client
        .post(url)
        .header("x-goog-api-key", token)
        .header("content-type", "application/json")
        .json(&req)
        .send()
        .map_err(|_| AiError::Network)?;

    let status = resp.status();
    if !status.is_success() {
        // PR #84 — read Gemini's error body and surface its
        // `error.message` so the UI shows e.g. "API key not valid"
        // or "Quota exceeded for ..." instead of "rate limited".
        let body = resp.bytes().map(|b| b.to_vec()).unwrap_or_default();
        return Err(map_response_error(status.as_u16(), body));
    }
    let body: GeminiResponse = resp.json().map_err(|_| AiError::Server(status.as_u16()))?;
    let text = body
        .candidates
        .into_iter()
        .next()
        .and_then(|c| c.content)
        .map(|content| {
            content
                .parts
                .into_iter()
                .map(|p| p.text)
                .collect::<Vec<_>>()
                .join("\n\n")
        })
        .unwrap_or_default();
    let (input_tokens, output_tokens) = body
        .usage_metadata
        .map(|u| (u.prompt_token_count, u.candidates_token_count))
        .unwrap_or((None, None));
    Ok(AiSuggestion {
        provider: Provider::Gemini,
        model: preview.model.clone(),
        generated_at: Utc::now(),
        suggestion_markdown: text,
        usage_input_tokens: input_tokens,
        usage_output_tokens: output_tokens,
    })
}

/// PR #84 — Consume a non-2xx response and surface the provider's
/// actual error message. Anthropic / OpenAI / Gemini all return
/// `error.message` (under different wrapping shapes) on failure;
/// reading it lets the UI distinguish "credit balance too low" from
/// "tokens-per-minute limit exceeded" from "model not available to
/// your tier" — every one of which the previous code collapsed into
/// "rate limited" for any 429.
///
/// Status-only fallbacks (401/403 → KeyInvalid, 429 → RateLimited,
/// etc.) still apply when the body can't be parsed, so a malformed
/// response from the provider doesn't lose the original signal.
fn map_response_error(status: u16, body_bytes: Vec<u8>) -> AiError {
    let body_text = String::from_utf8_lossy(&body_bytes);
    if let Some(message) = extract_provider_error_message(&body_text) {
        return AiError::ProviderError {
            status,
            message: cap_message(&message),
        };
    }
    // No structured body — fall back to the status-only classifier so
    // the user still gets the best information available.
    match status {
        401 | 403 => AiError::KeyInvalid,
        429 => AiError::RateLimited,
        _ => AiError::Server(status),
    }
}

/// Trim a provider-supplied error string so a runaway response can't
/// blow up the IPC error payload (or worse, ship megabytes of
/// arbitrary text into the UI). 600 chars is generous for the
/// realistic error messages the three providers emit.
fn cap_message(s: &str) -> String {
    const MAX: usize = 600;
    let trimmed = s.trim();
    if trimmed.chars().count() <= MAX {
        return trimmed.to_string();
    }
    let mut out: String = trimmed.chars().take(MAX).collect();
    out.push('…');
    out
}

/// Walk the three known provider error shapes looking for a string
/// `message` field:
///   * Anthropic / OpenAI / Gemini all use `{"error": {"message": "..."}}`
///   * Some Anthropic responses use the legacy `{"type":"error","error":{"message":"..."}}`
///     shape — the inner `error.message` path catches both.
///
/// Returns `None` if the body isn't JSON or no message string sits at
/// any recognized path; caller falls back to a status-only mapping.
fn extract_provider_error_message(body: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    // Common: {"error": {"message": "..."}}
    if let Some(msg) = json
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
    {
        return Some(msg.to_string());
    }
    // Top-level: {"message": "..."}
    if let Some(msg) = json.get("message").and_then(|m| m.as_str()) {
        return Some(msg.to_string());
    }
    // Anthropic legacy: {"type":"error","error":"<string>"}
    if let Some(msg) = json.get("error").and_then(|m| m.as_str()) {
        return Some(msg.to_string());
    }
    None
}

/// Production entry point used by the IPC bridge. Fetches the key,
/// dispatches through the production transport, then drops the key
/// before returning.
///
/// PR #74 — keys are now keyed by `provider_id` (each connected
/// provider has its own keychain slot). For backwards-compat with
/// legacy single-provider previews that lack `provider_id`, fall
/// back to the type-keyed slot.
pub fn send_with_provider_key(preview: &AiRequestPreview) -> Result<AiSuggestion, AiError> {
    let token = if preview.provider_id.is_empty() {
        key::get(preview.provider)?.ok_or(AiError::NoProviderKey)?
    } else {
        key::get_for_id(&preview.provider_id)?.ok_or(AiError::NoProviderKey)?
    };
    send_with(&ReqwestTransport, preview, &token)
}

/// Test seam — accepts an injected transport. Production callers use
/// `send_with_provider_key` above.
pub fn send_with(
    transport: &dyn Transport,
    preview: &AiRequestPreview,
    token: &Zeroizing<String>,
) -> Result<AiSuggestion, AiError> {
    transport.send(preview, token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_response_error_empty_body_falls_through_to_status_buckets() {
        assert!(matches!(
            map_response_error(401, vec![]),
            AiError::KeyInvalid
        ));
        assert!(matches!(
            map_response_error(403, vec![]),
            AiError::KeyInvalid
        ));
        assert!(matches!(
            map_response_error(429, vec![]),
            AiError::RateLimited
        ));
        assert!(matches!(
            map_response_error(500, vec![]),
            AiError::Server(500)
        ));
        assert!(matches!(
            map_response_error(503, vec![]),
            AiError::Server(503)
        ));
        assert!(matches!(
            map_response_error(418, vec![]),
            AiError::Server(418)
        ));
    }

    #[test]
    fn map_response_error_extracts_anthropic_message() {
        let body = br#"{"type":"error","error":{"type":"rate_limit_error","message":"Number of request tokens (45000) has exceeded your per-minute rate limit (40000)"}}"#;
        let err = map_response_error(429, body.to_vec());
        match err {
            AiError::ProviderError { status, message } => {
                assert_eq!(status, 429);
                assert!(message.contains("per-minute rate limit"));
            }
            other => panic!("expected ProviderError, got {other:?}"),
        }
    }

    #[test]
    fn map_response_error_extracts_openai_message() {
        let body = br#"{"error":{"message":"You exceeded your current quota","type":"insufficient_quota","code":"insufficient_quota"}}"#;
        let err = map_response_error(429, body.to_vec());
        match err {
            AiError::ProviderError { status, message } => {
                assert_eq!(status, 429);
                assert_eq!(message, "You exceeded your current quota");
            }
            other => panic!("expected ProviderError, got {other:?}"),
        }
    }

    #[test]
    fn map_response_error_extracts_gemini_message() {
        let body = br#"{"error":{"code":429,"message":"Quota exceeded for quota metric 'GenerateRequestsPerMinutePerProject'","status":"RESOURCE_EXHAUSTED"}}"#;
        let err = map_response_error(429, body.to_vec());
        match err {
            AiError::ProviderError { status, message } => {
                assert_eq!(status, 429);
                assert!(message.contains("Quota exceeded"));
            }
            other => panic!("expected ProviderError, got {other:?}"),
        }
    }

    #[test]
    fn map_response_error_unparseable_body_falls_back_to_status() {
        let err = map_response_error(
            429,
            b"<html>upstream returned a non-json body</html>".to_vec(),
        );
        assert!(matches!(err, AiError::RateLimited));
        let err = map_response_error(401, b"".to_vec());
        assert!(matches!(err, AiError::KeyInvalid));
        let err = map_response_error(503, b"oops".to_vec());
        assert!(matches!(err, AiError::Server(503)));
    }

    #[test]
    fn cap_message_truncates_long_strings() {
        let long = "x".repeat(700);
        let capped = cap_message(&long);
        assert!(
            capped.chars().count() <= 601,
            "len {}",
            capped.chars().count()
        );
        assert!(capped.ends_with('…'));
    }

    #[test]
    fn cap_message_passes_through_short_strings() {
        assert_eq!(cap_message("short"), "short");
        assert_eq!(cap_message("  trimmed  "), "trimmed");
    }
}
