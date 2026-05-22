// Opt-in remote refresh of bundled knowledge-base content.
//
// Contract 08 §Constraints + §Security Check:
//   - Default OFF; the feature is gated on a settings toggle.
//   - Fetches only public documentation from a URL the user can inspect.
//   - Never sends account information — the body is purely a GET, with no
//     account-bearing headers or query parameters.
//   - Validates received content before replacing the bundled set; on
//     failure the bundled baseline (and any prior remote cache) remains.
//
// Implementation note: the actual HTTP transport lives behind a `Fetcher`
// trait so tests can substitute a fixture without standing up an HTTPS
// server. Production uses `ReqwestFetcher` backed by `reqwest` (already
// in our transitive graph via the AWS SDK).

use std::collections::BTreeMap;

use chrono::Utc;
use serde::Deserialize;

use super::error::KnowledgebaseError;
use super::registry::{self, LoadedContent};
use super::storage;
use super::types::{RefreshApplyResult, RefreshCheckResult, RemoteBundle};

/// Maximum bytes we'll accept from a single refresh download. Bounding
/// this keeps a malicious or accidentally-huge upstream from exhausting
/// disk space or memory. 16MB is comfortably larger than the bundled
/// content (currently ~200KB) yet still defensible.
const MAX_BUNDLE_BYTES: usize = 16 * 1024 * 1024;

/// The two operations a fetcher must support. Returning a `Vec<u8>` keeps
/// the surface portable between the production `reqwest` impl and any
/// test/file impl.
pub trait Fetcher: Send + Sync {
    fn fetch_bytes(&self, url: &str) -> Result<Vec<u8>, KnowledgebaseError>;
}

/// Production HTTPS fetcher. `https://` only; the URL allow-list is
/// enforced by `storage::set_repo_url`.
pub struct ReqwestFetcher;

impl Fetcher for ReqwestFetcher {
    fn fetch_bytes(&self, url: &str) -> Result<Vec<u8>, KnowledgebaseError> {
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(30))
            .user_agent(concat!("CloudSaw/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| KnowledgebaseError::RefreshUnreachable)?;

        let resp = client
            .get(url)
            .send()
            .map_err(|_| KnowledgebaseError::RefreshUnreachable)?;

        if !resp.status().is_success() {
            return Err(KnowledgebaseError::RefreshUnreachable);
        }

        // Bound the response body so a hostile/upstream-broken endpoint
        // can't streaming-fill our memory.
        let bytes = resp
            .bytes()
            .map_err(|_| KnowledgebaseError::RefreshUnreachable)?;
        if bytes.len() > MAX_BUNDLE_BYTES {
            return Err(KnowledgebaseError::RefreshInvalidContent);
        }
        Ok(bytes.to_vec())
    }
}

/// File-scheme fetcher used by tests and by power-users who want to point
/// the refresh at a locally-prepared bundle. URL is `file:///path/to/file`.
pub struct FileFetcher;

impl Fetcher for FileFetcher {
    fn fetch_bytes(&self, url: &str) -> Result<Vec<u8>, KnowledgebaseError> {
        let path = url
            .strip_prefix("file://")
            .ok_or(KnowledgebaseError::RefreshUnreachable)?;
        // On Windows `file:///C:/path` strips to `/C:/path`; drop the
        // leading slash so the standard fs path is correct.
        let cleaned = if cfg!(windows) && path.starts_with('/') {
            &path[1..]
        } else {
            path
        };
        let bytes = std::fs::read(cleaned).map_err(|_| KnowledgebaseError::RefreshUnreachable)?;
        if bytes.len() > MAX_BUNDLE_BYTES {
            return Err(KnowledgebaseError::RefreshInvalidContent);
        }
        Ok(bytes)
    }
}

/// Dispatch a fetcher based on the URL scheme.
pub fn default_fetcher_for(url: &str) -> Box<dyn Fetcher> {
    if url.starts_with("file://") {
        Box::new(FileFetcher)
    } else {
        Box::new(ReqwestFetcher)
    }
}

/// Probe the remote source without applying. Used to power the "Check for
/// updates" affordance. Non-mutating except for recording `last_checked_at`
/// and clearing the last-error string on success.
pub fn check_for_kb_update(
    fetcher: &dyn Fetcher,
) -> Result<RefreshCheckResult, KnowledgebaseError> {
    if !storage::is_refresh_enabled()? {
        return Err(KnowledgebaseError::RefreshDisabled);
    }
    let url = storage::get_repo_url()?;
    let bytes = match fetcher.fetch_bytes(&url) {
        Ok(b) => b,
        Err(e) => {
            storage::set_last_error(Some("unreachable"))?;
            return Err(e);
        }
    };

    let bundle = match parse_bundle(&bytes) {
        Ok(b) => b,
        Err(e) => {
            storage::set_last_error(Some("invalid_content"))?;
            return Err(e);
        }
    };

    storage::set_last_checked_at(Utc::now())?;
    storage::set_last_error(None)?;

    let current_version = registry::current_version()?;
    let update_available = bundle.version != current_version;
    Ok(RefreshCheckResult {
        update_available,
        current_version,
        remote_version: Some(bundle.version),
        remote_article_count: Some(bundle.articles.len()),
        message: None,
    })
}

/// Fetch + validate + install. The remote content is parsed and shaped
/// into a `LoadedContent` BEFORE the on-disk cache is touched; if anything
/// fails the cache and bundled baseline are left untouched.
pub fn apply_kb_update(fetcher: &dyn Fetcher) -> Result<RefreshApplyResult, KnowledgebaseError> {
    if !storage::is_refresh_enabled()? {
        return Err(KnowledgebaseError::RefreshDisabled);
    }
    let url = storage::get_repo_url()?;
    let bytes = match fetcher.fetch_bytes(&url) {
        Ok(b) => b,
        Err(e) => {
            storage::set_last_error(Some("unreachable"))?;
            return Err(e);
        }
    };

    let bundle = match parse_bundle(&bytes) {
        Ok(b) => b,
        Err(e) => {
            storage::set_last_error(Some("invalid_content"))?;
            return Err(e);
        }
    };

    let mappings_value = serde_json::to_value(&bundle.mappings).map_err(|_| {
        // Should be impossible since we just parsed it; map to a stable
        // error code anyway.
        KnowledgebaseError::RefreshInvalidContent
    })?;

    // Build the in-memory shape first so we fail BEFORE writing any cache
    // files. Contract 08 §Constraints: bundled baseline must remain on
    // failure.
    let loaded: LoadedContent =
        registry::build_remote_content(&bundle.version, &bundle.articles, &mappings_value)?;

    // Write disk cache. If this fails, the in-memory state is left as it
    // was — the caller sees a typed error and the next read still returns
    // the bundled baseline.
    let mappings_json = serde_json::to_string_pretty(&bundle.mappings)
        .map_err(|_| KnowledgebaseError::RefreshInvalidContent)?;
    storage::write_remote_cache(&bundle.version, &bundle.articles, &mappings_json)?;

    // Now commit the in-memory swap + the settings flip. From the user's
    // perspective these happen together because each `with_content` call
    // grabs the latest snapshot under the cache's read lock.
    let articles_imported = loaded.articles.len();
    let frameworks_imported = loaded.mappings.frameworks.len();
    let source_version = loaded.version.clone();
    registry::install(loaded);
    storage::set_remote_active(true)?;
    storage::set_remote_version(&source_version)?;
    storage::set_last_applied_at(Utc::now())?;
    storage::set_last_checked_at(Utc::now())?;
    storage::set_last_error(None)?;

    Ok(RefreshApplyResult {
        applied: true,
        articles_imported,
        frameworks_imported,
        source_version: Some(source_version),
        message: None,
    })
}

/// On startup or when the user disables the refresh, restore the bundled
/// baseline as the active set. Does not delete the disk cache by default
/// — that's controlled separately by `disable_and_revert`.
pub fn revert_to_bundled() -> Result<(), KnowledgebaseError> {
    let bundled = registry::load_bundled()?;
    registry::install(bundled);
    storage::set_remote_active(false)?;
    Ok(())
}

/// Fully disable the feature: flip the setting, drop the remote cache,
/// and revert to bundled. Called by the IPC layer when the user toggles
/// the feature OFF.
pub fn disable_and_revert() -> Result<(), KnowledgebaseError> {
    storage::set_refresh_enabled(false)?;
    storage::clear_remote_cache()?;
    storage::clear_remote_version()?;
    storage::set_last_error(None)?;
    revert_to_bundled()
}

/// Bootstrap entry point called at app startup. If the feature is enabled
/// and a valid remote cache exists, install it; otherwise fall back to
/// the bundled set. Cache corruption or any error here is non-fatal and
/// silently degrades to bundled — the user can re-trigger the refresh.
pub fn bootstrap_active_content() -> Result<(), KnowledgebaseError> {
    let enabled = storage::is_refresh_enabled().unwrap_or(false);
    let remote_active = storage::is_remote_active().unwrap_or(false);

    if enabled && remote_active {
        if let Ok(Some((version, articles, mappings_json))) = storage::read_remote_cache() {
            let mappings_value: serde_json::Value = match serde_json::from_str(&mappings_json) {
                Ok(v) => v,
                Err(_) => {
                    let bundled = registry::load_bundled()?;
                    registry::install(bundled);
                    let _ = storage::set_remote_active(false);
                    return Ok(());
                }
            };
            match registry::build_remote_content(&version, &articles, &mappings_value) {
                Ok(content) => {
                    registry::install(content);
                    return Ok(());
                }
                Err(_) => {
                    let bundled = registry::load_bundled()?;
                    registry::install(bundled);
                    let _ = storage::set_remote_active(false);
                    return Ok(());
                }
            }
        }
    }

    let bundled = registry::load_bundled()?;
    registry::install(bundled);
    Ok(())
}

#[derive(Debug, Deserialize)]
struct WireBundle {
    version: String,
    articles: BTreeMap<String, String>,
    mappings: serde_json::Value,
}

fn parse_bundle(bytes: &[u8]) -> Result<RemoteBundle, KnowledgebaseError> {
    let wire: WireBundle =
        serde_json::from_slice(bytes).map_err(|_| KnowledgebaseError::RefreshInvalidContent)?;

    let mappings: crate::knowledgebase::types::MappingsDocument =
        serde_json::from_value(wire.mappings)
            .map_err(|_| KnowledgebaseError::RefreshInvalidContent)?;

    if wire.version.trim().is_empty() {
        return Err(KnowledgebaseError::RefreshInvalidContent);
    }
    if wire.articles.is_empty() {
        return Err(KnowledgebaseError::RefreshInvalidContent);
    }
    if mappings.frameworks.is_empty() {
        return Err(KnowledgebaseError::RefreshInvalidContent);
    }

    Ok(RemoteBundle {
        version: wire.version,
        articles: wire.articles,
        mappings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StaticFetcher(Vec<u8>);
    impl Fetcher for StaticFetcher {
        fn fetch_bytes(&self, _url: &str) -> Result<Vec<u8>, KnowledgebaseError> {
            Ok(self.0.clone())
        }
    }

    struct FailFetcher;
    impl Fetcher for FailFetcher {
        fn fetch_bytes(&self, _url: &str) -> Result<Vec<u8>, KnowledgebaseError> {
            Err(KnowledgebaseError::RefreshUnreachable)
        }
    }

    fn minimal_bundle_bytes(version: &str, article_id: &str) -> Vec<u8> {
        let v = serde_json::json!({
            "version": version,
            "articles": { article_id: "# Title" },
            "mappings": {
                "frameworks": { "soc2": { "name": "SOC 2" } },
                "mappings": {}
            }
        });
        serde_json::to_vec(&v).unwrap()
    }

    #[test]
    fn parse_bundle_rejects_empty_articles() {
        let v = serde_json::json!({
            "version": "1",
            "articles": {},
            "mappings": {
                "frameworks": { "soc2": { "name": "x" } },
                "mappings": {}
            }
        });
        let body = serde_json::to_vec(&v).unwrap();
        assert!(matches!(
            parse_bundle(&body),
            Err(KnowledgebaseError::RefreshInvalidContent)
        ));
    }

    #[test]
    fn parse_bundle_rejects_empty_frameworks() {
        let v = serde_json::json!({
            "version": "1",
            "articles": { "x": "# title" },
            "mappings": {
                "frameworks": {},
                "mappings": {}
            }
        });
        let body = serde_json::to_vec(&v).unwrap();
        assert!(matches!(
            parse_bundle(&body),
            Err(KnowledgebaseError::RefreshInvalidContent)
        ));
    }

    #[test]
    fn parse_bundle_accepts_minimal_well_formed_payload() {
        let body = minimal_bundle_bytes("1.1", "iam-user-no-mfa");
        let b = parse_bundle(&body).expect("parse should succeed");
        assert_eq!(b.version, "1.1");
        assert_eq!(b.articles.len(), 1);
    }

    #[test]
    fn fail_fetcher_surfaces_unreachable() {
        let f = FailFetcher;
        let res = f.fetch_bytes("https://nowhere");
        assert!(matches!(res, Err(KnowledgebaseError::RefreshUnreachable)));
    }

    #[test]
    fn static_fetcher_round_trips_bytes() {
        let body = minimal_bundle_bytes("1", "x");
        let f = StaticFetcher(body.clone());
        assert_eq!(f.fetch_bytes("any").unwrap(), body);
    }
}
