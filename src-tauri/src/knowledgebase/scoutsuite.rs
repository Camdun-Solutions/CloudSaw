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
    /// Upstream remediation text. Single sentence in most rules; a paragraph
    /// in some. Empty/absent for ~60% of rules — those return `None` from
    /// `get_entry`.
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
    // frontend can render a "Learn more" link list without needing a
    // typed field added to KnowledgeArticle (Contract 08 §Constraints
    // forward-compat surface).
    if let Some(refs) = entry.and_then(|e| e.references.as_ref()) {
        if !refs.is_empty() {
            article
                .unmatched_sections
                .entry("scoutsuite_references".to_string())
                .or_insert_with(|| refs.join("\n"));
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
}
