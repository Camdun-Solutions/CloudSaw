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
        return Err(map_status(status.as_u16()));
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
        return Err(map_status(status.as_u16()));
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
        return Err(map_status(status.as_u16()));
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

fn map_status(s: u16) -> AiError {
    match s {
        401 | 403 => AiError::KeyInvalid,
        429 => AiError::RateLimited,
        500..=599 => AiError::Server(s),
        _ => AiError::Server(s),
    }
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
    fn map_status_routes_known_codes() {
        assert!(matches!(map_status(401), AiError::KeyInvalid));
        assert!(matches!(map_status(403), AiError::KeyInvalid));
        assert!(matches!(map_status(429), AiError::RateLimited));
        assert!(matches!(map_status(500), AiError::Server(500)));
        assert!(matches!(map_status(503), AiError::Server(503)));
        assert!(matches!(map_status(418), AiError::Server(418)));
    }
}
