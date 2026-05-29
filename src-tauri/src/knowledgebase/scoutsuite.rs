// PR #81 — ScoutSuite rule metadata pull-through.
//
// ScoutSuite's bundled rule definitions (vendor/scoutsuite/.../rules/findings/
// *.json) carry three fields we want surfaced in the CloudSaw UI but that
// CloudSaw historically dropped on the floor:
//
//   * `remediation` — one or two sentences of upstream-authored remediation
//     guidance. Useful as a baseline for findings that don't have a
//     CloudSaw-bundled markdown article. Item 5 of the 2026-05-29 user
//     batch: "Every finding should have a recommended fix for basic
//     security. Tailored recommendations will come from the AI suggestion
//     layer when enabled."
//   * `references` — URLs to AWS docs / vendor write-ups. Surfaced as a
//     "Learn more" link list when present.
//   * `compliance` — CIS framework references (the only compliance frame-
//     work ScoutSuite consistently annotates). Folded into the existing
//     `ControlMapping` IPC as a synthesized `cis` framework entry when no
//     hand-authored mapping exists for the finding.
//
// All three are READ-ONLY for CloudSaw — we never modify the upstream
// data. The generator script lives at the repo root
// (`scripts/extract-scoutsuite-metadata.py` — see SPEC_NOTES.md). The
// resulting JSON file is committed to the repo and `include_str!`d here
// so the binary stays fully offline.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;

use super::types::{ControlReference, KnowledgeArticle};

/// Static raw JSON, generated from `vendor/scoutsuite/ScoutSuite/providers/
/// aws/rules/findings/*.json` by the extraction script. Bumped whenever the
/// vendored ScoutSuite source updates.
const SCOUTSUITE_METADATA_JSON: &str = include_str!("../../knowledgebase/scoutsuite_metadata.json");

/// Lazy-parsed map keyed by ScoutSuite rule_key (e.g. `iam-user-no-mfa`).
/// Re-using OnceLock keeps the parse cost paid exactly once per process.
static METADATA: OnceLock<BTreeMap<String, Entry>> = OnceLock::new();

#[derive(Debug, Clone, Deserialize)]
pub struct Entry {
    /// PR #83 — Display label for the article title slot. Generated
    /// at extraction time from the rule_key so every finding has a
    /// title-cased name even when no hand-authored markdown exists.
    #[serde(default)]
    pub name: Option<String>,
    /// PR #83 — CloudSaw-authored description paragraph. Surfaces in
    /// the Findings drawer when the bundled markdown article (if any)
    /// doesn't carry a `## Description` section.
    #[serde(default)]
    pub description: Option<String>,
    /// PR #83 — CloudSaw-authored risk-narrative paragraph. Same
    /// fallback semantics as `description`.
    #[serde(default)]
    pub risk: Option<String>,
    /// Upstream ScoutSuite remediation text. Single sentence in most
    /// rules; a paragraph in some. Empty/absent for ~60% of rules —
    /// those fall back to the service-keyed baseline generator.
    #[serde(default)]
    pub remediation: Option<String>,
    /// External URLs ScoutSuite ships alongside the rule definition.
    #[serde(default)]
    pub references: Option<Vec<String>>,
    /// CIS framework annotations (the only one ScoutSuite consistently
    /// tags). Each entry carries `name`, `version`, and `reference` —
    /// `reference` is the CIS control id (e.g. `1.13`).
    #[serde(default)]
    pub compliance: Option<Vec<ComplianceEntry>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ComplianceEntry {
    /// Framework name as ScoutSuite emits it. Example:
    /// "CIS Amazon Web Services Foundations".
    pub name: String,
    /// Framework version. Example: "1.2.0".
    #[serde(default)]
    pub version: Option<String>,
    /// Control identifier within the framework. Example: "2.4".
    pub reference: String,
}

fn metadata() -> &'static BTreeMap<String, Entry> {
    METADATA.get_or_init(|| {
        // Failure to parse the bundled JSON is a build-time defect — the
        // generator script + the `include_str!` together guarantee the
        // bytes are present and shape-stable. If a future refactor breaks
        // that we want a noisy panic at first call, not a silent empty
        // map.
        serde_json::from_str(SCOUTSUITE_METADATA_JSON)
            .expect("scoutsuite_metadata.json fails to parse — re-run scripts/extract-scoutsuite-metadata.py")
    })
}

/// Lookup metadata for a finding. Returns `None` if the rule isn't in the
/// extraction set or has no extractable fields.
pub fn get_entry(rule_key: &str) -> Option<&'static Entry> {
    metadata().get(rule_key)
}

/// Overlay ScoutSuite metadata onto a `KnowledgeArticle`. Hand-authored
/// fields always win; the upstream text only fills slots the article left
/// empty. PR #82 — the overlay ALSO promotes `matched` to `true` whenever
/// it leaves the article with a non-empty `remediation`, so the frontend
/// renders the article body instead of the "No remediation guidance yet"
/// boilerplate even when the bundled markdown set doesn't have a matching
/// article. Every finding now has a baseline remediation: hand-authored
/// → ScoutSuite upstream → service-keyed best-practices template.
///
/// What fills in:
///   * `remediation` ← `entry.remediation` (single-sentence baseline
///     from upstream), then a service-keyed best-practices paragraph if
///     neither the article nor ScoutSuite supply one
///   * `unmatched_sections.scoutsuite_references` ← `entry.references`
///     joined as a newline-delimited list (the frontend surfaces this as
///     a "Learn more" link block)
///
/// `terraform_fix` / `aws_cli_fix` / `false_positives` are intentionally
/// untouched — those slots are reserved for hand-authored content where
/// CloudSaw can speak with confidence; ScoutSuite doesn't ship them.
pub fn overlay_into_article(
    rule_key: &str,
    service: &str,
    mut article: KnowledgeArticle,
) -> KnowledgeArticle {
    let entry = get_entry(rule_key);
    // PR #83 — Title: prefer the metadata's display `name` over the
    // raw rule_key (which is what default_for stamps in). Hand-
    // authored articles override both via the markdown `# Title`
    // line, but for findings with no markdown article the metadata
    // name reads as a normal article header.
    if let Some(name) = entry.and_then(|e| e.name.as_deref()) {
        if !name.trim().is_empty() && article.title == rule_key {
            article.title = name.to_string();
        }
    }
    // PR #83 — Description / Risk: fill from generated metadata when
    // the hand-authored article doesn't supply them. Hand-authored
    // text always wins on these slots (the bundled markdown is
    // higher-quality prose than the generator's templated output);
    // metadata fills the gap for the 80+ findings without a bundled
    // article.
    if article.description.trim().is_empty() {
        if let Some(description) = entry.and_then(|e| e.description.as_deref()) {
            article.description = description.to_string();
        }
    }
    if article.risk.trim().is_empty() {
        if let Some(risk) = entry.and_then(|e| e.risk.as_deref()) {
            article.risk = risk.to_string();
        }
    }
    // Remediation: only overlay when the article doesn't already carry
    // a non-trivial value. The default fallback's "consult AWS docs"
    // paragraph counts as trivial (matched=false) and gets replaced;
    // an article whose remediation is empty whitespace gets replaced
    // too. A real article with content always wins.
    let should_overlay_remediation = !article.matched || article.remediation.trim().is_empty();
    if should_overlay_remediation {
        if let Some(remediation) = entry.and_then(|e| e.remediation.as_ref()) {
            article.remediation = remediation.clone();
        } else {
            // PR #82 — ScoutSuite has no upstream remediation either.
            // Fall back to a service-keyed best-practices baseline so
            // the "Remediation" tab is never empty. Tailored guidance
            // comes from the AI suggestion layer when the user enables
            // it; this is the offline baseline.
            article.remediation = generate_baseline_remediation(rule_key, service);
        }
    }
    // References: surface as a forward-compat unmatched_section so the
    // frontend can render a "References" link list without needing a
    // typed field added to KnowledgeArticle (Contract 08 §Constraints
    // forward-compat surface). PR #83 — renamed key from
    // `scoutsuite_references` to `References` so the section header
    // reads like a normal article H2, and each URL is rewritten to
    // markdown link syntax with a derived label (AWS hostnames get
    // a friendly "AWS docs — …" prefix; everything else falls back
    // to hostname + tail path segment) so SafeMarkdown renders them
    // as anchor tags instead of bare URLs.
    if let Some(refs) = entry.and_then(|e| e.references.as_ref()) {
        if !refs.is_empty() {
            let formatted = refs
                .iter()
                .map(|u| format!("- {}", format_reference_link(u)))
                .collect::<Vec<_>>()
                .join("\n");
            article
                .unmatched_sections
                .entry("References".to_string())
                .or_insert(formatted);
        }
    }
    // PR #82 — flip matched=true if the overlay produced any useful
    // content. The frontend uses this flag to choose between
    // <ArticleBody> (article rendering) and <NoArticleBlock> (sad-path
    // "No KB article yet"). After this PR every finding renders the
    // article body because every finding has a remediation.
    if !article.remediation.trim().is_empty() {
        article.matched = true;
    }
    article
}

/// PR #83 — derive a descriptive markdown link label from a raw URL.
/// ScoutSuite ships references as bare URLs (`https://docs.aws.amazon.com/
/// IAM/latest/UserGuide/...`). Rendering them as anchor tags requires the
/// SafeMarkdown link syntax `[label](url)`; this builds that string with a
/// human-readable label so the rendered links read like prose links, not
/// pasted URLs.
///
/// Strategy:
///   * AWS docs hosts → "AWS Documentation — <last path segment>"
///   * AWS marketing site → "AWS — <last path segment>"
///   * Anything else → "<hostname> — <last path segment>" with the .com
///     stripped so the chip reads as a brand name
///
/// The last path segment is unslugified (dashes/underscores → spaces,
/// `.html` suffix trimmed, title-cased) so a URL like `.../password-best-
/// practices.html` lands as "Password best practices". When the URL has
/// no path tail, the hostname alone is used as the label.
fn format_reference_link(url: &str) -> String {
    let (host, last_segment) = split_url(url);
    let pretty_tail = pretty_segment(last_segment);
    let label = match host.as_deref() {
        Some(h) if h.contains("docs.aws.amazon.com") => {
            if pretty_tail.is_empty() {
                "AWS Documentation".to_string()
            } else {
                format!("AWS Documentation — {pretty_tail}")
            }
        }
        Some(h) if h.ends_with("aws.amazon.com") => {
            if pretty_tail.is_empty() {
                "AWS".to_string()
            } else {
                format!("AWS — {pretty_tail}")
            }
        }
        Some(h) => {
            let brand = h
                .trim_start_matches("www.")
                .trim_end_matches(".com")
                .trim_end_matches(".org")
                .to_string();
            if pretty_tail.is_empty() {
                brand
            } else {
                format!("{brand} — {pretty_tail}")
            }
        }
        None => url.to_string(),
    };
    format!("[{label}]({url})")
}

/// Parse a URL into (host, last_meaningful_path_segment). Returns
/// `(None, "")` when the URL doesn't look parseable — caller falls back
/// to using the raw URL as the label.
fn split_url(url: &str) -> (Option<String>, &str) {
    // strip scheme
    let after_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let (authority, path) = after_scheme.split_once('/').unwrap_or((after_scheme, ""));
    let host = if authority.is_empty() {
        None
    } else {
        Some(authority.to_ascii_lowercase())
    };
    // last non-empty segment, ignoring fragments + query strings.
    let path = path.split('?').next().unwrap_or(path);
    let path = path.split('#').next().unwrap_or(path);
    let last = path.rsplit('/').find(|s| !s.is_empty()).unwrap_or("");
    (host, last)
}

/// Turn a path segment like `password-best-practices.html` into
/// "Password best practices". Drops common HTML/PDF extensions, swaps
/// separators to spaces, and capitalizes the first character.
fn pretty_segment(seg: &str) -> String {
    if seg.is_empty() {
        return String::new();
    }
    let trimmed = seg
        .trim_end_matches(".html")
        .trim_end_matches(".htm")
        .trim_end_matches(".pdf")
        .trim_end_matches(".aspx");
    let spaced: String = trimmed
        .chars()
        .map(|c| if c == '-' || c == '_' { ' ' } else { c })
        .collect();
    let trimmed = spaced.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let mut chars = trimmed.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// PR #82 — generate a service-keyed best-practices baseline when neither
/// the bundled KB nor ScoutSuite has remediation text for the rule. The
/// templates intentionally read as starting-point advice: enough to give
/// the user a direction, not so specific that they'd be misled into
/// applying the wrong fix. Tailored guidance comes from the AI suggestion
/// layer; this is the offline floor.
fn generate_baseline_remediation(rule_key: &str, service: &str) -> String {
    let domain_advice = match service {
        "iam" | "organizations" | "sso" | "cognito" => {
            "Review the flagged IAM resources and apply the principle of \
             least privilege. Use IAM Access Analyzer to identify external \
             access paths, require MFA for all human principals, rotate \
             long-lived credentials regularly, and prefer roles + temporary \
             credentials over IAM users where possible."
        }
        "s3" => {
            "Verify the S3 bucket configuration against AWS security \
             baselines: enable Block Public Access at the account and \
             bucket level, default-encrypt with SSE-S3 or SSE-KMS, enforce \
             HTTPS-only access via bucket policy, enable access logging, \
             and turn on versioning + MFA Delete for buckets holding \
             durable state."
        }
        "ec2" | "vpc" => {
            "Audit the security-group ingress rules — avoid `0.0.0.0/0` on \
             SSH, RDP, and database ports. Enable VPC Flow Logs on every \
             VPC, place workloads in private subnets behind a NAT or VPC \
             endpoint, and use SCPs to prevent default-VPC creation across \
             the org."
        }
        "elasticloadbalancing" | "elb" | "elbv2" | "cloudfront" | "apigateway" => {
            "Confirm TLS configuration: require TLS 1.2 or higher, attach \
             AWS-managed security policies that exclude weak ciphers, and \
             redirect HTTP to HTTPS at the listener. Enable access logs to \
             S3 with retention aligned to your compliance window."
        }
        "cloudtrail" | "cloudwatch" | "config" => {
            "Verify the CloudTrail / Config / CloudWatch baseline: a \
             multi-region CloudTrail with KMS-encrypted log files, log-file \
             validation on, CloudTrail integrated into CloudWatch Logs + \
             alarms for sensitive API calls, and AWS Config rules covering \
             the controls your compliance framework expects."
        }
        "kms" | "secretsmanager" | "acm" => {
            "Tighten the key/secret policy — grant access only to specific \
             IAM principals (avoid `Principal: \"*\"`), enable automatic \
             rotation, tag by environment so audits stay readable, and \
             restrict cross-account access via the key policy AND IAM."
        }
        "rds" | "dynamodb" | "redshift" | "backup" => {
            "Confirm the data store's security posture: encryption at rest \
             with a customer-managed KMS key, automatic backups + Point-in- \
             Time Recovery, deletion protection on production instances, no \
             public accessibility, and database authentication via IAM (or \
             SSO) rather than local credentials."
        }
        "guardduty" | "securityhub" | "inspector" | "macie" => {
            "Enable the detective control in every region for the active \
             account (and across the AWS Organization if applicable), \
             route findings to a central security account via EventBridge \
             + Security Hub, and tune the noise floor so high-severity \
             findings actually wake an on-call."
        }
        "lambda" | "ecs" | "ecr" | "eks" => {
            "Verify the compute workload's security posture: execution role \
             with least privilege, runtime updates tracked, image scanning \
             enabled at the registry, secrets injected from Secrets \
             Manager or Parameter Store (not env-vars), and outbound \
             network egress reviewed."
        }
        "cloudformation" => {
            "Review the CloudFormation stack configuration — restrict \
             template parameter overrides, set DeletionPolicy: Retain on \
             stateful resources, use stack policies to block unintended \
             updates, and enable termination protection on production \
             stacks."
        }
        _ => {
            "Review the resources flagged for this finding, consult the \
             relevant AWS service documentation, and apply least-privilege \
             changes to bring the configuration in line with the security \
             baseline you've established for this environment."
        }
    };
    format!(
        "**Baseline best-practices guidance for `{rule_key}`.** {domain_advice}\n\n\
         *No tailored remediation is bundled for this finding. Enable the \
         AI Suggestion Layer in Settings to generate a tailored fix that \
         takes your business context into account.*"
    )
}

/// Synthesize a list of `ControlReference` entries from a finding's CIS
/// compliance annotations. Returns an empty vec when the finding has no
/// CIS entries — callers should then omit the `cis` framework from the
/// `ControlMapping` they return.
pub fn cis_controls_for(rule_key: &str) -> Vec<ControlReference> {
    let entry = match get_entry(rule_key) {
        Some(e) => e,
        None => return Vec::new(),
    };
    let compliance = match &entry.compliance {
        Some(c) => c,
        None => return Vec::new(),
    };
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for c in compliance {
        // Only CIS annotations; ScoutSuite occasionally tags other
        // frameworks but their coverage is too sparse to be useful here
        // and our hand-authored mappings.json covers SOC2/ISO27001/HIPAA/
        // NIST already.
        if !c.name.contains("CIS") {
            continue;
        }
        let version_tag = c.version.as_deref().unwrap_or("?");
        let control_id = format!("CIS {version_tag} §{}", c.reference);
        if !seen.insert(control_id.clone()) {
            continue;
        }
        let title = format!("Mapped from {}", c.name);
        out.push(ControlReference { control_id, title });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_parses_at_call_time() {
        // Calling `metadata()` once forces the OnceLock to run — if the
        // bundled JSON has shape drift the panic fires here, not in
        // production at first use.
        let m = metadata();
        assert!(!m.is_empty(), "scoutsuite_metadata.json is empty");
    }

    #[test]
    fn cis_controls_for_unknown_rule_returns_empty() {
        // Defensive: don't blow up on a rule_key the extraction set
        // doesn't cover.
        assert!(cis_controls_for("not-a-real-rule").is_empty());
    }

    #[test]
    fn format_reference_link_aws_docs() {
        let s = format_reference_link(
            "https://docs.aws.amazon.com/IAM/latest/UserGuide/id_credentials_passwords_account-policy.html",
        );
        assert_eq!(
            s,
            "[AWS Documentation — Id credentials passwords account policy](https://docs.aws.amazon.com/IAM/latest/UserGuide/id_credentials_passwords_account-policy.html)"
        );
    }

    #[test]
    fn format_reference_link_securityhub_with_fragment() {
        let s = format_reference_link(
            "https://docs.aws.amazon.com/securityhub/latest/userguide/securityhub-cis-controls.html#securityhub-cis-controls-1.11",
        );
        assert!(s.starts_with("[AWS Documentation — "));
        assert!(s.contains("Securityhub cis controls"));
    }

    #[test]
    fn format_reference_link_external() {
        let s = format_reference_link("https://owasp.org/www-project-top-ten/");
        assert_eq!(
            s,
            "[owasp — Www project top ten](https://owasp.org/www-project-top-ten/)"
        );
    }

    #[test]
    fn format_reference_link_no_path() {
        let s = format_reference_link("https://aws.amazon.com");
        assert_eq!(s, "[AWS](https://aws.amazon.com)");
    }
}
