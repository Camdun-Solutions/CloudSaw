// Request builder — redaction by CONSTRUCTION (Contract 13).
//
// The builder reads a finding from the local store and emits a request
// that contains ONLY:
//
//   * `rule_key` — a finding-TYPE token (e.g. `s3-public-bucket`).
//     This is a scanner slug, never an account- or resource-specific
//     value.
//   * `service` — an AWS service name (`s3`, `iam`, …) — a category.
//   * `severity` — one of five fixed enum values.
//   * `checked_items` / `flagged_items` — counts, never IDs.
//   * `resource_category` — derived from `rule_key` (`bucket`, `role`,
//     `security_group`, …). Computed; never read from a resource path.
//
// Where the prompt text needs a placeholder for a specific resource
// (e.g. "block public access on bucket [REDACTED-RESOURCE-NAME]"), the
// builder uses a CONSTANT placeholder string and records it in
// `placeholders_used`. There is NO real-value↔placeholder map. The
// builder does not load resource paths, the request does not transmit
// them, and the response handler does not swap them back.
//
// CLAUDE.md §5 hard-DO-NOT — the request body is the only thing this
// module ever produces, and it is the same thing the user sees in the
// preview modal. What you preview IS what gets sent.

use super::context;
use super::error::AiError;
use super::types::{
    AiRequestPreview, BusinessContext, EnvironmentType, FindingDigest, Provider, RiskTolerance,
    TeamSize,
};
use crate::findings;

/// Constant placeholder labels. These strings travel through the
/// request body and the response unchanged. The list is the only
/// data-flow channel between the request and any subsequent display
/// — and it carries category labels, not real-value mappings.
pub const PLACEHOLDER_RESOURCE: &str = "[REDACTED-RESOURCE-NAME]";
pub const PLACEHOLDER_BUCKET: &str = "[REDACTED-BUCKET-NAME]";
pub const PLACEHOLDER_ROLE: &str = "[REDACTED-ROLE-NAME]";
pub const PLACEHOLDER_ACCOUNT: &str = "[REDACTED-ACCOUNT-ID]";
pub const PLACEHOLDER_ARN: &str = "[REDACTED-ARN]";

/// Default model per provider. The user doesn't pick a model in this
/// version — the field is exposed in the preview so the user can see
/// what they're talking to.
const ANTHROPIC_DEFAULT_MODEL: &str = "claude-haiku-4-5-20251001";
const OPENAI_DEFAULT_MODEL: &str = "gpt-4o-mini";
// PR #77 — Gemini default. `gemini-2.0-flash` is the cost-tier model
// in the v1beta API surface; same scale and latency posture as
// Anthropic Haiku / OpenAI gpt-4o-mini.
const GEMINI_DEFAULT_MODEL: &str = "gemini-2.0-flash";

/// Build the preview the UI must show to the user. The result is the
/// EXACT payload that would be sent to the provider — there is no
/// further rewriting in the client.
pub fn build_preview(
    provider: Provider,
    provider_id: &str,
    finding_id: &str,
) -> Result<AiRequestPreview, AiError> {
    if finding_id.len() != 64 || !finding_id.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(AiError::InvalidInput("finding_id"));
    }
    let detail = findings::get_finding(finding_id).map_err(|e| match e {
        findings::FindingsError::FindingNotFound => AiError::FindingNotFound,
        other => AiError::Db(other.to_string()),
    })?;

    let category = resource_category_for(&detail.finding.rule_key, &detail.finding.service);
    let digest = FindingDigest {
        rule_key: detail.finding.rule_key.clone(),
        service: detail.finding.service.clone(),
        severity: severity_label(detail.finding.severity).to_string(),
        checked_items: detail.finding.checked_items,
        flagged_items: detail.finding.flagged_items,
        resource_category: category.to_string(),
    };

    let ctx = context::read_context()?;
    let flags = context::flag_fields(&ctx);

    let placeholders_static = placeholders_for(category);
    let placeholders: Vec<String> = placeholders_static.iter().map(|s| s.to_string()).collect();

    let system_prompt = render_system_prompt();
    let user_message = render_user_message(&digest, &ctx, &placeholders_static);

    let model = match provider {
        Provider::Anthropic => ANTHROPIC_DEFAULT_MODEL,
        Provider::Openai => OPENAI_DEFAULT_MODEL,
        Provider::Gemini => GEMINI_DEFAULT_MODEL,
    };

    Ok(AiRequestPreview {
        provider,
        provider_id: provider_id.to_string(),
        model: model.to_string(),
        system_prompt,
        user_message,
        digest,
        context: ctx,
        flags,
        placeholders_used: placeholders,
    })
}

/// Map a rule key + service to a high-level resource category. Pure
/// function — never reads from the finding's `resource_path` field.
fn resource_category_for(rule_key: &str, service: &str) -> &'static str {
    let k = rule_key.to_ascii_lowercase();
    if k.contains("bucket") || service == "s3" {
        return "bucket";
    }
    if k.contains("security-group") || k.contains("security_group") {
        return "security_group";
    }
    if k.contains("role") {
        return "role";
    }
    if k.contains("user") {
        return "user";
    }
    if k.contains("policy") {
        return "policy";
    }
    if k.contains("instance") || service == "ec2" {
        return "instance";
    }
    if k.contains("distribution") || service == "cloudfront" {
        return "distribution";
    }
    if k.contains("trail") || service == "cloudtrail" {
        return "trail";
    }
    if k.contains("password") {
        return "password_policy";
    }
    "resource"
}

fn placeholders_for(category: &'static str) -> Vec<&'static str> {
    match category {
        "bucket" => vec![PLACEHOLDER_BUCKET],
        "role" => vec![PLACEHOLDER_ROLE],
        // Generic per-resource placeholder for everything else.
        _ => vec![PLACEHOLDER_RESOURCE],
    }
}

fn severity_label(s: findings::Severity) -> &'static str {
    match s {
        findings::Severity::Critical => "critical",
        findings::Severity::High => "high",
        findings::Severity::Medium => "medium",
        findings::Severity::Low => "low",
        findings::Severity::Informational => "informational",
    }
}

fn render_system_prompt() -> String {
    // Constant template — never includes per-call substitution.
    "You are CloudSaw's AI Suggestion Layer. Your job is to suggest \
     concrete remediation steps for AWS misconfiguration findings.\n\n\
     Rules you MUST follow:\n\
     1. Treat the request as describing a CATEGORY of resource. Any name \
        the user references appears as a constant placeholder like \
        [REDACTED-BUCKET-NAME]; keep placeholders as placeholders in your \
        reply.\n\
     2. Do NOT ask for or invent specific account IDs, ARNs, or resource \
        names. Speak in terms of categories.\n\
     3. Tailor the suggestion to the supplied business context \
        (environment type, compliance, risk tolerance, team size).\n\
     4. Be concise: one short remediation paragraph plus, when useful, \
        a single fenced code block of Terraform or CLI.\n\
     5. Clearly call out residual risks and assumptions."
        .to_string()
}

fn render_user_message(
    digest: &FindingDigest,
    ctx: &BusinessContext,
    placeholders: &[&'static str],
) -> String {
    let mut s = String::new();
    s.push_str("## Finding (category-level only)\n\n");
    s.push_str(&format!("- Rule key: `{}`\n", digest.rule_key));
    s.push_str(&format!("- Service category: `{}`\n", digest.service));
    s.push_str(&format!(
        "- Resource category: `{}`\n",
        digest.resource_category
    ));
    s.push_str(&format!("- Severity: `{}`\n", digest.severity));
    s.push_str(&format!(
        "- Resources of this type: checked {} / flagged {}\n",
        digest.checked_items, digest.flagged_items
    ));
    s.push('\n');
    s.push_str("## Business context (user-supplied, structured)\n\n");
    s.push_str(&format!("- Industry: {}\n", none_if_empty(&ctx.industry)));
    s.push_str(&format!(
        "- Job role / use case: {}\n",
        none_if_empty(&ctx.job_role)
    ));
    s.push_str(&format!(
        "- Environment type: {}\n",
        env_label(ctx.environment_type)
    ));
    s.push_str(&format!(
        "- Compliance obligations: {}\n",
        if ctx.compliance.is_empty() {
            "(none specified)".to_string()
        } else {
            ctx.compliance.join(", ")
        }
    ));
    s.push_str(&format!(
        "- Risk tolerance: {}\n",
        risk_label(ctx.risk_tolerance)
    ));
    s.push_str(&format!("- Team size: {}\n", team_label(ctx.team_size)));
    s.push('\n');
    s.push_str("## Placeholders you may see and MUST keep as-is\n\n");
    for ph in placeholders {
        s.push_str(&format!("- `{ph}`\n"));
    }
    s.push('\n');
    s.push_str(
        "Please suggest concrete remediation steps. Keep any placeholder \
         tokens unchanged in your response.",
    );
    s
}

fn none_if_empty(s: &str) -> String {
    if s.trim().is_empty() {
        "(none specified)".to_string()
    } else {
        s.trim().to_string()
    }
}

fn env_label(e: EnvironmentType) -> &'static str {
    match e {
        EnvironmentType::Production => "production",
        EnvironmentType::DevTest => "dev/test",
        EnvironmentType::Mixed => "mixed",
        EnvironmentType::Unspecified => "(none specified)",
    }
}
fn risk_label(r: RiskTolerance) -> &'static str {
    match r {
        RiskTolerance::Low => "low",
        RiskTolerance::Medium => "medium",
        RiskTolerance::High => "high",
        RiskTolerance::Unspecified => "(none specified)",
    }
}
fn team_label(t: TeamSize) -> &'static str {
    match t {
        TeamSize::Solo => "solo",
        TeamSize::Small => "small",
        TeamSize::Medium => "medium",
        TeamSize::Large => "large",
        TeamSize::Unspecified => "(none specified)",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_category_buckets_route_to_bucket() {
        assert_eq!(resource_category_for("s3-public-bucket", "s3"), "bucket");
        assert_eq!(resource_category_for("iam-role-no-mfa", "iam"), "role");
        assert_eq!(
            resource_category_for("ec2-security-group-opens-ssh-to-all", "ec2"),
            "security_group"
        );
    }

    #[test]
    fn placeholders_for_bucket_use_bucket_token() {
        assert_eq!(placeholders_for("bucket"), vec![PLACEHOLDER_BUCKET]);
        assert_eq!(placeholders_for("role"), vec![PLACEHOLDER_ROLE]);
        assert_eq!(placeholders_for("instance"), vec![PLACEHOLDER_RESOURCE]);
    }

    #[test]
    fn rendered_user_message_never_contains_a_raw_arn_or_account_id() {
        let digest = FindingDigest {
            rule_key: "s3-public-bucket".into(),
            service: "s3".into(),
            severity: "high".into(),
            checked_items: 12,
            flagged_items: 3,
            resource_category: "bucket".into(),
        };
        let ctx = BusinessContext {
            industry: "fintech".into(),
            job_role: "".into(),
            environment_type: EnvironmentType::Production,
            compliance: vec!["PCI".into(), "SOC2".into()],
            risk_tolerance: RiskTolerance::Low,
            team_size: TeamSize::Small,
        };
        let msg = render_user_message(&digest, &ctx, &placeholders_for("bucket"));
        // The body MUST NOT contain an ARN, an account ID, or a real
        // bucket name (we never put one in). The placeholder MUST appear.
        assert!(msg.contains(PLACEHOLDER_BUCKET));
        assert!(!msg.contains("arn:aws:"));
        assert!(!msg.contains("111122223333"));
        // Business context is reflected.
        assert!(msg.contains("fintech"));
        assert!(msg.contains("production"));
        assert!(msg.contains("PCI"));
    }
}
