// In-memory cache for articles, mappings, and the framework list.
//
// Loaded once on first access (lazy initialization keyed on the active
// source). Re-initialization happens when the remote refresh flips the
// active set; the cache rebuild swaps both maps atomically so callers
// either see "fully bundled" or "fully remote", never a half-applied
// patchwork (Contract 08 §Constraints: "no per-request disk reads").

use std::collections::BTreeMap;
use std::sync::{OnceLock, RwLock};

use serde_json::Value;

use super::bundled::{BUNDLED_ARTICLES, BUNDLED_MAPPINGS_JSON, BUNDLED_VERSION};
use super::error::KnowledgebaseError;
use super::parser;
use super::types::{
    ArticleSummary, ControlMapping, ControlReference, Framework, KnowledgeArticle, KnowledgeSource,
    MappingsDocument,
};

const MAX_ARTICLE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub struct ArticleEntry {
    pub body: String,
    pub title: String,
}

#[derive(Debug, Clone)]
pub struct LoadedContent {
    pub source: KnowledgeSource,
    pub version: String,
    pub articles: BTreeMap<String, ArticleEntry>,
    pub mappings: MappingsDocument,
}

fn cache() -> &'static RwLock<Option<LoadedContent>> {
    static C: OnceLock<RwLock<Option<LoadedContent>>> = OnceLock::new();
    C.get_or_init(|| RwLock::new(None))
}

/// Initialize (or re-initialize) the cache. Called at app startup with the
/// active source from disk, and again when the user applies a refresh.
pub fn install(content: LoadedContent) {
    let mut guard = cache().write().expect("kb cache poisoned");
    *guard = Some(content);
}

/// Drop the in-memory cache. Used by tests to force a fresh load between
/// scenarios. Production callers never invoke this.
#[doc(hidden)]
pub fn reset_for_tests() {
    let mut guard = cache().write().expect("kb cache poisoned");
    *guard = None;
}

/// Ensure the cache holds the bundled baseline. Idempotent: subsequent
/// calls return without reloading unless `force = true`.
pub fn ensure_bundled_loaded(force: bool) -> Result<(), KnowledgebaseError> {
    if !force {
        let read = cache().read().expect("kb cache poisoned");
        if read.is_some() {
            return Ok(());
        }
    }
    let content = load_bundled()?;
    install(content);
    Ok(())
}

/// Look up the active content, building the bundled baseline on demand.
/// Every public read goes through this so first-call lazy init still
/// satisfies the "cached in memory" Constraint.
fn with_content<R>(f: impl FnOnce(&LoadedContent) -> R) -> Result<R, KnowledgebaseError> {
    ensure_bundled_loaded(false)?;
    let read = cache().read().expect("kb cache poisoned");
    let content = read
        .as_ref()
        .ok_or(KnowledgebaseError::Internal("registry empty after init"))?;
    Ok(f(content))
}

pub fn current_source() -> Result<KnowledgeSource, KnowledgebaseError> {
    with_content(|c| c.source)
}

pub fn current_version() -> Result<String, KnowledgebaseError> {
    with_content(|c| c.version.clone())
}

pub fn get_article(finding_id: &str) -> Result<KnowledgeArticle, KnowledgebaseError> {
    with_content(|c| match c.articles.get(finding_id) {
        Some(entry) => parser::parse_article(finding_id, c.source, &entry.body),
        None => KnowledgeArticle::default_for(finding_id, c.source),
    })
}

pub fn list_articles() -> Result<Vec<ArticleSummary>, KnowledgebaseError> {
    with_content(|c| {
        c.articles
            .iter()
            .map(|(id, entry)| ArticleSummary {
                finding_id: id.clone(),
                title: entry.title.clone(),
                source: c.source,
            })
            .collect()
    })
}

pub fn get_control_mappings(finding_id: &str) -> Result<ControlMapping, KnowledgebaseError> {
    with_content(|c| {
        let mut frameworks: BTreeMap<String, Vec<ControlReference>> = BTreeMap::new();
        if let Some(by_framework) = c.mappings.mappings.get(finding_id) {
            for (framework_id, controls) in by_framework {
                frameworks.insert(framework_id.clone(), controls.clone());
            }
        }
        ControlMapping {
            finding_id: finding_id.to_string(),
            frameworks,
        }
    })
}

pub fn list_frameworks() -> Result<Vec<Framework>, KnowledgebaseError> {
    with_content(|c| {
        c.mappings
            .frameworks
            .iter()
            .map(|(id, def)| Framework {
                id: id.clone(),
                name: def.name.clone(),
            })
            .collect()
    })
}

/// Build a `LoadedContent` from the compile-time bundled set. Public so
/// the refresh layer can fall back to the bundled baseline after a failed
/// remote apply.
pub fn load_bundled() -> Result<LoadedContent, KnowledgebaseError> {
    // Duplicate detection on the bundled list. Contract 08 §Constraints:
    // ambiguity is a startup error. The list is sorted by construction;
    // verifying it here keeps the bundled.rs maintainability check honest.
    let mut prev: Option<&str> = None;
    for (id, _) in BUNDLED_ARTICLES {
        if let Some(p) = prev {
            if p == *id {
                return Err(KnowledgebaseError::DuplicateArticleId((*id).to_string()));
            }
            // Sort order is enforced explicitly so list_articles is stable
            // and so the duplicate-check above only needs to look back one
            // step.
            if p > *id {
                return Err(KnowledgebaseError::Internal(
                    "bundled.rs articles out of order",
                ));
            }
        }
        prev = Some(*id);
    }

    let mut articles: BTreeMap<String, ArticleEntry> = BTreeMap::new();
    for (id, body) in BUNDLED_ARTICLES {
        if body.len() > MAX_ARTICLE_BYTES {
            // Soft-warn at startup; the build-time signal lives in build.rs
            // (which writes cargo::warning lines so the warning is visible
            // during `cargo build`). Either signal points the maintainer
            // at the file to trim.
            eprintln!(
                "knowledgebase: bundled article '{}' is {} bytes (>{}); consider trimming",
                id,
                body.len(),
                MAX_ARTICLE_BYTES,
            );
        }
        let title = parser::extract_title(id, body);
        articles.insert(
            (*id).to_string(),
            ArticleEntry {
                body: (*body).to_string(),
                title,
            },
        );
    }

    let mappings: MappingsDocument =
        parse_mappings(BUNDLED_MAPPINGS_JSON).map_err(KnowledgebaseError::MalformedMappings)?;

    Ok(LoadedContent {
        source: KnowledgeSource::Bundled,
        version: BUNDLED_VERSION.to_string(),
        articles,
        mappings,
    })
}

/// Build a `LoadedContent` from a remote-bundle payload. Validation lives
/// here so the refresh layer cannot install a malformed cache that would
/// crash later lookups.
pub fn build_remote_content(
    version: &str,
    raw_articles: &BTreeMap<String, String>,
    mappings_json: &Value,
) -> Result<LoadedContent, KnowledgebaseError> {
    if version.trim().is_empty() {
        return Err(KnowledgebaseError::RefreshInvalidContent);
    }
    if raw_articles.is_empty() {
        return Err(KnowledgebaseError::RefreshInvalidContent);
    }

    let mut articles: BTreeMap<String, ArticleEntry> = BTreeMap::new();
    for (id, body) in raw_articles {
        if id.trim().is_empty() {
            return Err(KnowledgebaseError::RefreshInvalidContent);
        }
        if !is_valid_finding_id(id) {
            return Err(KnowledgebaseError::RefreshInvalidContent);
        }
        if articles.contains_key(id) {
            return Err(KnowledgebaseError::DuplicateArticleId(id.clone()));
        }
        if body.len() > MAX_ARTICLE_BYTES {
            // Soft warn; remote content over the limit is suspicious but
            // not by itself an integrity failure.
            eprintln!(
                "knowledgebase: remote article '{}' is {} bytes (>{}); trimming may be needed",
                id,
                body.len(),
                MAX_ARTICLE_BYTES,
            );
        }
        let title = parser::extract_title(id, body);
        articles.insert(
            id.clone(),
            ArticleEntry {
                body: body.clone(),
                title,
            },
        );
    }

    let mappings_str = serde_json::to_string(mappings_json)
        .map_err(|e| KnowledgebaseError::MalformedMappings(e.to_string()))?;
    let mappings: MappingsDocument =
        parse_mappings(&mappings_str).map_err(|_| KnowledgebaseError::RefreshInvalidContent)?;

    Ok(LoadedContent {
        source: KnowledgeSource::Remote,
        version: version.to_string(),
        articles,
        mappings,
    })
}

fn parse_mappings(s: &str) -> Result<MappingsDocument, String> {
    serde_json::from_str(s).map_err(|e| e.to_string())
}

/// Finding IDs follow the ScoutSuite style: lowercase ASCII with hyphens.
/// The remote path rejects anything else so a corrupted manifest can't
/// inject `..` or path-shaped IDs.
fn is_valid_finding_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_set_loads_without_duplicates() {
        let content = load_bundled().expect("bundled load should succeed");
        // Sanity: at least the originally-promised 30+ articles ship.
        assert!(content.articles.len() >= 30);
        assert_eq!(content.source, KnowledgeSource::Bundled);
        assert!(content.mappings.frameworks.contains_key("soc2"));
        assert!(content.mappings.frameworks.contains_key("iso27001"));
        assert!(content.mappings.frameworks.contains_key("hipaa"));
        assert!(content.mappings.frameworks.contains_key("nist"));
    }

    #[test]
    fn valid_finding_id_accepts_scoutsuite_style() {
        assert!(is_valid_finding_id("iam-password-policy-no-minimum-length"));
        assert!(!is_valid_finding_id(""));
        assert!(!is_valid_finding_id("iam/path"));
        assert!(!is_valid_finding_id("../etc/passwd"));
        assert!(!is_valid_finding_id("IAM-UPPER"));
    }

    #[test]
    fn remote_content_with_empty_articles_is_rejected() {
        let mappings = serde_json::json!({"frameworks": {}, "mappings": {}});
        let articles: BTreeMap<String, String> = BTreeMap::new();
        let err = build_remote_content("1.0.0", &articles, &mappings).unwrap_err();
        matches!(err, KnowledgebaseError::RefreshInvalidContent);
    }

    #[test]
    fn remote_content_with_traversal_id_is_rejected() {
        let mappings = serde_json::json!({"frameworks": {}, "mappings": {}});
        let mut articles = BTreeMap::new();
        articles.insert("../escape".to_string(), "# x".to_string());
        let err = build_remote_content("1.0.0", &articles, &mappings).unwrap_err();
        matches!(err, KnowledgebaseError::RefreshInvalidContent);
    }
}
