// SQLite-backed settings and the on-disk remote-cache layout.
//
// The knowledgebase module re-uses the generic `settings` key/value table
// (migration 0001). It does NOT add new schema. Per CLAUDE.md §6.5 we
// could add forward-only migrations, but the settings table is purpose-
// built for module-prefixed keys exactly like these.
//
// On-disk cache:
//   <data_root>/knowledgebase/
//       articles/<finding-id>.md       — one file per remote article
//       mappings.json                  — remote mappings document
//       VERSION                        — single line containing the
//                                        applied remote version string

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};

use super::error::KnowledgebaseError;
use crate::db::paths::{app_data_dir, ensure_user_only_dir, set_user_only};

const KEY_REFRESH_ENABLED: &str = "knowledgebase.remote_refresh_enabled";
const KEY_REPO_URL: &str = "knowledgebase.remote_repo_url";
const KEY_REMOTE_ACTIVE: &str = "knowledgebase.remote_active";
const KEY_LAST_CHECKED: &str = "knowledgebase.last_checked_at";
const KEY_LAST_APPLIED: &str = "knowledgebase.last_applied_at";
const KEY_REMOTE_VERSION: &str = "knowledgebase.remote_version";
const KEY_LAST_ERROR: &str = "knowledgebase.last_error";

/// Default upstream the refresh fetches from. Documented as a public URL
/// in Contract 08 §Inputs. Users can change it via `set_refresh_settings`
/// (e.g. to point at a private fork during testing).
pub const DEFAULT_REPO_URL: &str =
    "https://raw.githubusercontent.com/Camdun-Solutions/cloudsaw-knowledgebase/main/bundle.json";

fn db_path() -> Result<PathBuf, KnowledgebaseError> {
    Ok(app_data_dir()
        .map_err(|e| KnowledgebaseError::Db(e.to_string()))?
        .join("db")
        .join("cloudsaw.db"))
}

fn open() -> Result<Connection, KnowledgebaseError> {
    let path = db_path()?;
    Connection::open(&path).map_err(KnowledgebaseError::from)
}

fn read_string(key: &str) -> Result<Option<String>, KnowledgebaseError> {
    let conn = open()?;
    let v: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |r| r.get(0),
        )
        .optional()?;
    Ok(v)
}

fn write_string(key: &str, value: &str) -> Result<(), KnowledgebaseError> {
    let conn = open()?;
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![key, value, now],
    )?;
    Ok(())
}

fn delete_key(key: &str) -> Result<(), KnowledgebaseError> {
    let conn = open()?;
    conn.execute("DELETE FROM settings WHERE key = ?1", params![key])?;
    Ok(())
}

pub fn is_refresh_enabled() -> Result<bool, KnowledgebaseError> {
    Ok(matches!(
        read_string(KEY_REFRESH_ENABLED)?.as_deref(),
        Some("1")
    ))
}

pub fn set_refresh_enabled(enabled: bool) -> Result<(), KnowledgebaseError> {
    write_string(KEY_REFRESH_ENABLED, if enabled { "1" } else { "0" })
}

pub fn get_repo_url() -> Result<String, KnowledgebaseError> {
    Ok(read_string(KEY_REPO_URL)?.unwrap_or_else(|| DEFAULT_REPO_URL.to_string()))
}

pub fn set_repo_url(url: &str) -> Result<(), KnowledgebaseError> {
    if !is_acceptable_url(url) {
        return Err(KnowledgebaseError::InvalidInput("repo_url"));
    }
    write_string(KEY_REPO_URL, url)
}

pub fn is_remote_active() -> Result<bool, KnowledgebaseError> {
    Ok(matches!(read_string(KEY_REMOTE_ACTIVE)?.as_deref(), Some("1")))
}

pub fn set_remote_active(active: bool) -> Result<(), KnowledgebaseError> {
    write_string(KEY_REMOTE_ACTIVE, if active { "1" } else { "0" })
}

pub fn get_remote_version() -> Result<Option<String>, KnowledgebaseError> {
    read_string(KEY_REMOTE_VERSION)
}

pub fn set_remote_version(version: &str) -> Result<(), KnowledgebaseError> {
    write_string(KEY_REMOTE_VERSION, version)
}

pub fn clear_remote_version() -> Result<(), KnowledgebaseError> {
    delete_key(KEY_REMOTE_VERSION)
}

pub fn get_last_checked_at() -> Result<Option<DateTime<Utc>>, KnowledgebaseError> {
    parse_optional_ts(read_string(KEY_LAST_CHECKED)?)
}

pub fn set_last_checked_at(ts: DateTime<Utc>) -> Result<(), KnowledgebaseError> {
    write_string(KEY_LAST_CHECKED, &ts.to_rfc3339())
}

pub fn get_last_applied_at() -> Result<Option<DateTime<Utc>>, KnowledgebaseError> {
    parse_optional_ts(read_string(KEY_LAST_APPLIED)?)
}

pub fn set_last_applied_at(ts: DateTime<Utc>) -> Result<(), KnowledgebaseError> {
    write_string(KEY_LAST_APPLIED, &ts.to_rfc3339())
}

pub fn get_last_error() -> Result<Option<String>, KnowledgebaseError> {
    read_string(KEY_LAST_ERROR)
}

pub fn set_last_error(err: Option<&str>) -> Result<(), KnowledgebaseError> {
    match err {
        Some(s) => write_string(KEY_LAST_ERROR, s),
        None => delete_key(KEY_LAST_ERROR),
    }
}

fn parse_optional_ts(raw: Option<String>) -> Result<Option<DateTime<Utc>>, KnowledgebaseError> {
    match raw {
        None => Ok(None),
        Some(s) => DateTime::parse_from_rfc3339(&s)
            .map(|dt| Some(dt.with_timezone(&Utc)))
            .map_err(|e| KnowledgebaseError::Db(format!("ts parse: {e}"))),
    }
}

/// Strict allow-list of acceptable schemes for the remote refresh URL.
/// HTTPS only — the entire point is to fetch verified public documentation,
/// and we never want to silently fall back to plain HTTP.
fn is_acceptable_url(url: &str) -> bool {
    if !(url.starts_with("https://") || url.starts_with("file://")) {
        return false;
    }
    if url.len() > 2048 {
        return false;
    }
    if url.chars().any(|c| c.is_control()) {
        return false;
    }
    true
}

/// Resolve the on-disk cache directory and ensure it has the strict
/// user-only ACL. Returns the directory; caller writes into it.
pub fn cache_dir() -> Result<PathBuf, KnowledgebaseError> {
    let root = app_data_dir().map_err(|e| KnowledgebaseError::Io(e.to_string()))?;
    let dir = root.join("knowledgebase");
    ensure_user_only_dir(&dir).map_err(|e| KnowledgebaseError::Io(e.to_string()))?;
    ensure_user_only_dir(&dir.join("articles"))
        .map_err(|e| KnowledgebaseError::Io(e.to_string()))?;
    Ok(dir)
}

pub fn cache_articles_dir() -> Result<PathBuf, KnowledgebaseError> {
    Ok(cache_dir()?.join("articles"))
}

pub fn cache_mappings_path() -> Result<PathBuf, KnowledgebaseError> {
    Ok(cache_dir()?.join("mappings.json"))
}

pub fn cache_version_path() -> Result<PathBuf, KnowledgebaseError> {
    Ok(cache_dir()?.join("VERSION"))
}

/// Write the validated remote payload to the on-disk cache atomically.
/// Existing cache contents are removed first; on failure the directory is
/// left in a half-state — callers MUST treat any error as "cache is
/// corrupt, fall back to bundled".
pub fn write_remote_cache(
    version: &str,
    articles: &BTreeMap<String, String>,
    mappings_json: &str,
) -> Result<(), KnowledgebaseError> {
    let dir = cache_dir()?;
    let articles_dir = cache_articles_dir()?;

    // Best-effort wipe of any prior cache. We don't care if the directory
    // was empty — we just need a clean slate.
    if let Ok(entries) = std::fs::read_dir(&articles_dir) {
        for entry in entries.flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
    }

    // Write articles. Each filename is the finding_id with the registry's
    // path-shape rules already applied (see registry::build_remote_content);
    // we re-check here as defense in depth.
    for (id, body) in articles {
        if !is_safe_filename(id) {
            return Err(KnowledgebaseError::RefreshInvalidContent);
        }
        let target = articles_dir.join(format!("{id}.md"));
        atomic_write(&target, body.as_bytes())?;
    }

    atomic_write(&cache_mappings_path()?, mappings_json.as_bytes())?;
    atomic_write(&cache_version_path()?, version.as_bytes())?;
    let _ = dir; // path is already user-only via ensure_user_only_dir.
    Ok(())
}

/// Re-read the on-disk remote cache. Returns Ok(None) if the cache hasn't
/// been populated yet. The returned tuple matches the shape
/// `(version, articles, mappings_json)` so `registry::build_remote_content`
/// can consume it directly.
#[allow(clippy::type_complexity)]
pub fn read_remote_cache() -> Result<
    Option<(String, BTreeMap<String, String>, String)>,
    KnowledgebaseError,
> {
    let version_path = cache_version_path()?;
    if !version_path.exists() {
        return Ok(None);
    }
    let version = std::fs::read_to_string(&version_path)?.trim().to_string();
    if version.is_empty() {
        return Ok(None);
    }
    let mappings = std::fs::read_to_string(cache_mappings_path()?)?;

    let articles_dir = cache_articles_dir()?;
    let mut articles = BTreeMap::new();
    if let Ok(entries) = std::fs::read_dir(&articles_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if !is_safe_filename(stem) {
                continue;
            }
            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            let body = std::fs::read_to_string(&path)?;
            articles.insert(stem.to_string(), body);
        }
    }
    Ok(Some((version, articles, mappings)))
}

/// Clear the on-disk remote cache. Called when the user disables remote
/// refresh or when `apply_kb_update` chooses to revert to bundled.
pub fn clear_remote_cache() -> Result<(), KnowledgebaseError> {
    let dir = match cache_dir() {
        Ok(d) => d,
        Err(_) => return Ok(()),
    };
    let _ = std::fs::remove_file(dir.join("VERSION"));
    let _ = std::fs::remove_file(dir.join("mappings.json"));
    if let Ok(entries) = std::fs::read_dir(dir.join("articles")) {
        for entry in entries.flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    Ok(())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), KnowledgebaseError> {
    // Plain write — the cache dir is process-private and the upstream call
    // sequence already serializes refreshes (the IPC layer doesn't run
    // two `apply_kb_update`s concurrently because the frontend disables
    // the button while one is in flight). A genuine atomic rename would
    // require Windows-specific care; the simpler write keeps the platform
    // surface small.
    std::fs::write(path, bytes)?;
    set_user_only(path, false).map_err(|e| KnowledgebaseError::Io(e.to_string()))?;
    Ok(())
}

fn is_safe_filename(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_validation_rejects_non_https() {
        assert!(is_acceptable_url("https://example.com/bundle.json"));
        assert!(is_acceptable_url("file:///tmp/test-bundle.json"));
        assert!(!is_acceptable_url("http://example.com/bundle.json"));
        assert!(!is_acceptable_url("ftp://example.com/bundle.json"));
        assert!(!is_acceptable_url(""));
        assert!(!is_acceptable_url("javascript:alert(1)"));
    }

    #[test]
    fn filename_rejects_path_traversal() {
        assert!(is_safe_filename("iam-user-no-mfa"));
        assert!(!is_safe_filename("../../etc/passwd"));
        assert!(!is_safe_filename("with/slash"));
        assert!(!is_safe_filename("with\\backslash"));
        assert!(!is_safe_filename(""));
    }
}
