// Public data types crossing the IPC boundary for the knowledgebase module.
//
// CLAUDE.md §4.1: IPC payloads are plain serializable structs — no internal
// rusqlite/reqwest/serde_json types and no credential-bearing material. The
// markdown payloads are raw strings; rendering (and sanitization) happens in
// the frontend (Contract 09).

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Where the currently-active article set originates. Bundled = compiled-in
/// at build time. Remote = pulled by an explicit user-opt-in refresh and
/// cached on disk. Per Contract 08 §Constraints, the bundled set is always
/// available as an offline fallback even when Remote is selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeSource {
    Bundled,
    Remote,
}

impl KnowledgeSource {
    pub fn as_str(self) -> &'static str {
        match self {
            KnowledgeSource::Bundled => "bundled",
            KnowledgeSource::Remote => "remote",
        }
    }
}

/// One knowledge-base article. Per Contract 08 §Expected Output: each H2
/// section becomes a typed field; unrecognized H2 sections survive in
/// `unmatched_sections` so future drafts (e.g. "Compliance Mapping") don't
/// silently vanish before code learns about them.
///
/// `matched = false` plus the empty defaults below is the answer the API
/// returns for a finding that has no article yet — never an error. This
/// keeps the UI uniform: every finding has *something* to render.
#[derive(Debug, Clone, Serialize)]
pub struct KnowledgeArticle {
    pub finding_id: String,
    pub matched: bool,
    pub source: KnowledgeSource,
    pub title: String,
    pub description: String,
    pub risk: String,
    pub detection_logic: String,
    pub remediation: String,
    pub terraform_fix: String,
    pub aws_cli_fix: String,
    pub false_positives: String,
    /// Unexpected H2 sections, in the order they appeared. Forward-compat
    /// surface — added so a future article can introduce a new section
    /// without breaking older app builds.
    pub unmatched_sections: BTreeMap<String, String>,
}

impl KnowledgeArticle {
    /// The default-article shape returned when no matching markdown file
    /// exists. PR #82 — `description` is left EMPTY in the default and
    /// the upstream-overlay path (`scoutsuite::overlay_into_article`)
    /// fills `remediation` from ScoutSuite or a service-keyed baseline,
    /// then flips `matched = true` so the article body renders. The
    /// frontend filters out empty sections, so a default-article with
    /// only `remediation` populated still renders cleanly. Callers
    /// should NOT rely on `matched = false` as a sad-path signal; the
    /// overlay almost always promotes it to true.
    pub fn default_for(finding_id: &str, source: KnowledgeSource) -> Self {
        KnowledgeArticle {
            finding_id: finding_id.to_string(),
            matched: false,
            source,
            title: finding_id.to_string(),
            description: String::new(),
            risk: String::new(),
            detection_logic: String::new(),
            // Default remediation is intentionally empty — the overlay
            // step is the single source of truth for what populates this
            // field. Letting the default fill in here would mask the
            // upstream pull-through that PR #82 wires up.
            remediation: String::new(),
            terraform_fix: String::new(),
            aws_cli_fix: String::new(),
            false_positives: String::new(),
            unmatched_sections: BTreeMap::new(),
        }
    }
}

/// One row in `list_articles` — enough to render an article picker without
/// shipping every article's body to the UI.
#[derive(Debug, Clone, Serialize)]
pub struct ArticleSummary {
    pub finding_id: String,
    pub title: String,
    pub source: KnowledgeSource,
}

/// Control mapping for a finding across all bundled frameworks. Frameworks
/// the finding has no entries in are omitted; an empty vec for a present
/// framework means "we mapped this finding to that framework with zero
/// matching controls" (rare; the more common case is omission).
#[derive(Debug, Clone, Serialize)]
pub struct ControlMapping {
    pub finding_id: String,
    /// Framework id (e.g. `soc2`, `iso27001`) → list of control entries.
    /// BTreeMap so the order across calls is stable for snapshot tests.
    pub frameworks: BTreeMap<String, Vec<ControlReference>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlReference {
    pub control_id: String,
    pub title: String,
}

/// One supported compliance framework. The id is the lower-case slug used as
/// the key in `ControlMapping.frameworks`; `name` is the display string.
#[derive(Debug, Clone, Serialize)]
pub struct Framework {
    pub id: String,
    pub name: String,
}

/// Settings for the opt-in remote refresh. `enabled = false` by default —
/// Contract 08 §Constraints and §Security Check.
///
/// `repo_url` is the source the refresh pulls from; it's exposed (read-only
/// from the frontend's perspective; the user changes it via the dedicated
/// settings command) so the UI can show "downloads from <url>" copy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshSettings {
    pub enabled: bool,
    pub repo_url: String,
    pub remote_active: bool,
    pub last_checked_at: Option<DateTime<Utc>>,
    pub last_applied_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
}

/// Result of `check_for_kb_update` — a non-mutating probe of the remote
/// source. Failure does not modify state; the UI shows the message and the
/// user decides whether to retry.
#[derive(Debug, Clone, Serialize)]
pub struct RefreshCheckResult {
    pub update_available: bool,
    pub current_version: String,
    pub remote_version: Option<String>,
    pub remote_article_count: Option<usize>,
    pub message: Option<String>,
}

/// Result of `apply_kb_update` — a state-mutating refresh. On failure the
/// bundled baseline (and any prior remote cache) is preserved untouched.
#[derive(Debug, Clone, Serialize)]
pub struct RefreshApplyResult {
    pub applied: bool,
    pub articles_imported: usize,
    pub frameworks_imported: usize,
    pub source_version: Option<String>,
    pub message: Option<String>,
}

/// Update to apply via `set_refresh_settings`. Both fields optional so the
/// caller can toggle the feature without clobbering the repo URL (or vice
/// versa). Settings only — never includes secrets.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct RefreshSettingsUpdate {
    pub enabled: Option<bool>,
    pub repo_url: Option<String>,
}

/// Internal serialized format of the bundled mappings.json file. Kept
/// public so tests can construct fixtures without recreating the schema in
/// every file. New frameworks land here as additional top-level keys — no
/// code change required (Contract 08 §Edge Cases).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MappingsDocument {
    pub frameworks: BTreeMap<String, FrameworkDefinition>,
    /// finding_id -> framework_id -> list of control entries.
    pub mappings: BTreeMap<String, BTreeMap<String, Vec<ControlReference>>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FrameworkDefinition {
    pub name: String,
}

/// Wire format of a remote refresh payload. Same shape as the bundled
/// content, plus a version string that drives "update available?".
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RemoteBundle {
    pub version: String,
    pub articles: BTreeMap<String, String>,
    pub mappings: MappingsDocument,
}
