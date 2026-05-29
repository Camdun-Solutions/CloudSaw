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
const SCOUTSUITE_METADATA_JSON: &str =
    include_str!("../../knowledgebase/scoutsuite_metadata.json");

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
/// empty.
///
/// What fills in:
///   * `remediation`         ← `entry.remediation` (one sentence baseline)
///   * `unmatched_sections.scoutsuite_references` ← `entry.references`
///     joined as a newline-delimited list (the frontend surfaces this as
///     a "Learn more" link block)
///
/// `terraform_fix` / `aws_cli_fix` / `false_positives` are intentionally
/// untouched — those slots are reserved for hand-authored content where
/// CloudSaw can speak with confidence; ScoutSuite doesn't ship them.
pub fn overlay_into_article(rule_key: &str, mut article: KnowledgeArticle) -> KnowledgeArticle {
    let entry = match get_entry(rule_key) {
        Some(e) => e,
        None => return article,
    };
    // Remediation: only overlay when the article doesn't already carry
    // a non-trivial value. The default fallback's "consult AWS docs"
    // paragraph counts as trivial (matched=false) and gets replaced;
    // an article whose remediation is empty whitespace gets replaced
    // too. A real article with content always wins.
    let should_overlay_remediation =
        !article.matched || article.remediation.trim().is_empty();
    if should_overlay_remediation {
        if let Some(remediation) = entry.remediation.as_ref() {
            article.remediation = remediation.clone();
        }
    }
    // References: surface as a forward-compat unmatched_section so the
    // frontend can render a "Learn more" link list without needing a
    // typed field added to KnowledgeArticle (Contract 08 §Constraints
    // forward-compat surface).
    if let Some(refs) = entry.references.as_ref() {
        if !refs.is_empty() {
            article
                .unmatched_sections
                .entry("scoutsuite_references".to_string())
                .or_insert_with(|| refs.join("\n"));
        }
    }
    article
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
