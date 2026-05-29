// Public types for the AI Suggestion Layer. Every value here is safe
// for both IPC and the request-preview modal. No real ARNs, no account
// IDs, no resource identifiers — by construction (see `builder.rs`).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Which provider the user has connected. Exactly one is active at a
/// time. `None` means the feature is dormant (no provider chosen).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Anthropic,
    Openai,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Provider::Anthropic => "anthropic",
            Provider::Openai => "openai",
        }
    }

    pub fn from_storage(s: &str) -> Option<Self> {
        match s {
            "anthropic" => Some(Provider::Anthropic),
            "openai" => Some(Provider::Openai),
            _ => None,
        }
    }
}

/// Discrete environment type the user attests to. Drives prompt tuning
/// (e.g. "production" raises severity of cross-cutting findings).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentType {
    Production,
    DevTest,
    Mixed,
    #[default]
    Unspecified,
}

impl EnvironmentType {
    pub fn as_str(self) -> &'static str {
        match self {
            EnvironmentType::Production => "production",
            EnvironmentType::DevTest => "dev_test",
            EnvironmentType::Mixed => "mixed",
            EnvironmentType::Unspecified => "",
        }
    }

    pub fn from_storage(s: &str) -> EnvironmentType {
        match s {
            "production" => EnvironmentType::Production,
            "dev_test" => EnvironmentType::DevTest,
            "mixed" => EnvironmentType::Mixed,
            _ => EnvironmentType::Unspecified,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskTolerance {
    Low,
    Medium,
    High,
    #[default]
    Unspecified,
}

impl RiskTolerance {
    pub fn as_str(self) -> &'static str {
        match self {
            RiskTolerance::Low => "low",
            RiskTolerance::Medium => "medium",
            RiskTolerance::High => "high",
            RiskTolerance::Unspecified => "",
        }
    }

    pub fn from_storage(s: &str) -> RiskTolerance {
        match s {
            "low" => RiskTolerance::Low,
            "medium" => RiskTolerance::Medium,
            "high" => RiskTolerance::High,
            _ => RiskTolerance::Unspecified,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamSize {
    Solo,
    Small,
    Medium,
    Large,
    #[default]
    Unspecified,
}

impl TeamSize {
    pub fn as_str(self) -> &'static str {
        match self {
            TeamSize::Solo => "solo",
            TeamSize::Small => "small",
            TeamSize::Medium => "medium",
            TeamSize::Large => "large",
            TeamSize::Unspecified => "",
        }
    }

    pub fn from_storage(s: &str) -> TeamSize {
        match s {
            "solo" => TeamSize::Solo,
            "small" => TeamSize::Small,
            "medium" => TeamSize::Medium,
            "large" => TeamSize::Large,
            _ => TeamSize::Unspecified,
        }
    }
}

/// Structured business context the user fills in once and the request
/// builder reuses on every call. None of these fields are mirrored
/// elsewhere (no logs, no event log content). The UI flags any free-
/// form field whose value could be identifying ("industry") so the
/// user sees what would be sent.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BusinessContext {
    pub industry: String,
    /// PR #69 — Job role / "what the user uses CloudSaw for". Up to
    /// 500 characters (validated in `write_context`). Surfaces in
    /// the AI prompt builder so suggestions can take the user's
    /// role into account (e.g. "I'm the lone SRE for a mid-size
    /// SaaS"). Free-form; the user owns whether to disclose
    /// anything identifying.
    #[serde(default)]
    pub job_role: String,
    pub environment_type: EnvironmentType,
    pub compliance: Vec<String>,
    pub risk_tolerance: RiskTolerance,
    pub team_size: TeamSize,
}

/// Whether a context field looks identifying — surfaced to the UI so
/// the user sees a "this will be sent to your AI provider" warning
/// next to the field AND in the request preview.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextFlags {
    /// Industry is free-form; non-empty values are flagged because a
    /// user might type "Acme Corp Healthcare" thinking it's just a
    /// label.
    pub industry_identifying: bool,
    /// Compliance entries are flagged if any value looks like a
    /// specific identifier (mixed-case + digits without recognized
    /// framework keywords).
    pub compliance_identifying: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AiSettings {
    pub provider: Option<Provider>,
    pub key_connected: bool,
    pub context: BusinessContext,
    pub flags: ContextFlags,
}

/// Bundle of finding-shaped data the request builder reads. Every
/// field here is a NORMALIZED CATEGORY, never a raw identifier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindingDigest {
    /// Stable rule slug (e.g. `s3-public-bucket`). This is a finding-
    /// TYPE token, not an account-, resource-, or org-specific value.
    pub rule_key: String,
    /// AWS service name (`s3`, `iam`, `ec2`, …) — a category, not a
    /// resource.
    pub service: String,
    /// One of `critical|high|medium|low|informational`.
    pub severity: String,
    /// Number of resources of this type that exist for the account
    /// vs. the number that were flagged. Counts, never IDs.
    pub checked_items: i64,
    pub flagged_items: i64,
    /// Resource CATEGORY only — e.g. `bucket`, `role`, `security_group`.
    /// Derived from `rule_key`, not from a resource path.
    pub resource_category: String,
}

/// One AI suggestion request prior to send. The IPC bridge returns
/// this from `prepare`; the UI displays it verbatim in the preview
/// modal; the same value is passed to `send`. What you see IS what
/// gets transmitted — there is no last-mile rewriting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiRequestPreview {
    pub provider: Provider,
    /// PR #74 — `provider_id` of the connected provider row whose
    /// keychain slot will be used to authorize this request. Required
    /// for the multi-provider model: the same `provider` type may have
    /// several rows, each with its own key. The send path reads the
    /// key from `cloudsaw.llm_api_key` with `account = provider_id`.
    #[serde(default)]
    pub provider_id: String,
    pub model: String,
    /// The system prompt the provider receives. Constant template;
    /// no per-call substitution other than the digest values below.
    pub system_prompt: String,
    /// The user-role message: finding digest + business context,
    /// rendered as a deterministic markdown template. No raw IDs.
    pub user_message: String,
    /// The exact digest the user message was built from — surfaced
    /// separately so the UI can show the line-by-line "what we're
    /// telling the AI" table.
    pub digest: FindingDigest,
    pub context: BusinessContext,
    pub flags: ContextFlags,
    /// Constant placeholder labels the user message uses for any
    /// resource-shaped string. Surfaced separately so the UI can
    /// render a "placeholders stay placeholders" reminder under the
    /// preview.
    pub placeholders_used: Vec<String>,
}

/// One AI suggestion response. Plain markdown the UI renders through
/// the sanitized markdown component, with a clear "AI-generated,
/// unreviewed" label above. No swap-back occurs: placeholders the
/// model echoed back stay as placeholders.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiSuggestion {
    pub provider: Provider,
    pub model: String,
    pub generated_at: DateTime<Utc>,
    pub suggestion_markdown: String,
    /// Approximate token usage if the provider returned it. Used by
    /// the UI to render a "x tokens" hint so the user can size their
    /// own provider bill. Not load-bearing.
    pub usage_input_tokens: Option<u32>,
    pub usage_output_tokens: Option<u32>,
}
