// PR #74 — Multi-provider AI storage.
//
// One row in `ai_providers` per connected provider. Each row records:
//
//   * `provider_id`   — random hex slug; also the keychain `account`
//                       slot under `cloudsaw.llm_api_key`. Migrated
//                       legacy rows (single-provider model) keep their
//                       account name (`anthropic` | `openai`) as the
//                       `provider_id` so the keychain key transfers
//                       without re-prompting the user.
//   * `provider_type` — `anthropic` | `openai`.
//   * `nickname`      — user-facing label. Whitespace-trimmed,
//                       1-60 chars. Duplicate nicknames are allowed
//                       (the id is what disambiguates) but the UI
//                       discourages them.
//   * `key_last4`     — last four characters of the real API key,
//                       for the "****ABCD" display. The real key
//                       NEVER lives in SQLite — only in the keychain.
//
// The currently-active provider id sits in `settings.ai_active_provider_id`.
// Empty string = "no provider active" (the layer is dormant).
//
// Contract 11 (panic wipe) reads `list()` to enumerate every provider
// row so the corresponding keychain entries can be removed. The static
// `KEYCHAIN_OWNED_ENTRIES` list in `keychain::mod.rs` covers the two
// legacy slots; dynamic per-id slots are deleted in `delete()` here
// and during panic wipe via the listing path.

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use super::error::AiError;
use super::key;
use super::types::Provider;
use crate::db::paths::app_data_dir;

const ACTIVE_PROVIDER_KEY: &str = "ai_active_provider_id";

/// Public record returned to the IPC frontend. Carries everything the
/// UI needs to render a Connected-Provider row except the real key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRecord {
    pub provider_id: String,
    pub provider_type: Provider,
    pub nickname: String,
    pub key_last4: String,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// Maximum nickname length the storage layer accepts. The UI shows a
/// counter; values above the cap are rejected here as defense in depth.
pub const NICKNAME_MAX_LEN: usize = 60;

fn db_path() -> Result<std::path::PathBuf, AiError> {
    Ok(app_data_dir()
        .map_err(|e| AiError::Io(e.to_string()))?
        .join("db")
        .join("cloudsaw.db"))
}

fn open() -> Result<Connection, AiError> {
    Connection::open(db_path()?).map_err(AiError::from)
}

fn read_setting(conn: &Connection, key_name: &str) -> Result<String, AiError> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key_name],
            |r| r.get(0),
        )
        .optional()?;
    Ok(raw.unwrap_or_default())
}

fn write_setting(conn: &Connection, key_name: &str, value: &str) -> Result<(), AiError> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO settings (key, value, updated_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value,
                                        updated_at = excluded.updated_at",
        params![key_name, value, now],
    )?;
    Ok(())
}

/// Compute the last-4 display from a real API key. The string is
/// truncated to its tail; values shorter than 4 chars (the shape
/// check would reject these earlier, but defense in depth) pad with
/// the dot leader so the UI never crashes.
fn key_last4(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() <= 4 {
        chars.iter().collect()
    } else {
        chars[chars.len() - 4..].iter().collect()
    }
}

/// Generate a random hex provider id. 16 hex chars = 8 random bytes,
/// which has enough entropy for the small number of provider rows a
/// single user will accumulate.
fn new_provider_id() -> Result<String, AiError> {
    let conn = open()?;
    let id: String = conn
        .query_row("SELECT lower(hex(randomblob(8)))", [], |r| r.get(0))
        .map_err(AiError::from)?;
    Ok(id)
}

fn row_to_record(row: &rusqlite::Row<'_>, active_id: &str) -> rusqlite::Result<ProviderRecord> {
    let provider_id: String = row.get(0)?;
    let provider_type_raw: String = row.get(1)?;
    let provider_type = Provider::from_storage(&provider_type_raw).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            1,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::other(format!(
                "unknown provider_type '{provider_type_raw}'"
            ))),
        )
    })?;
    let nickname: String = row.get(2)?;
    let key_last4: String = row.get(3)?;
    let created_at: String = row.get(4)?;
    let updated_at: String = row.get(5)?;
    let is_active = !active_id.is_empty() && active_id == provider_id;
    Ok(ProviderRecord {
        provider_id,
        provider_type,
        nickname,
        key_last4,
        is_active,
        created_at,
        updated_at,
    })
}

/// List every connected provider, ordered by creation time (oldest
/// first) so the UI shows providers in the order the user added them.
pub fn list() -> Result<Vec<ProviderRecord>, AiError> {
    let conn = open()?;
    let active_id = read_setting(&conn, ACTIVE_PROVIDER_KEY)?;
    let mut stmt = conn.prepare(
        "SELECT provider_id, provider_type, nickname, key_last4, created_at, updated_at
         FROM ai_providers
         ORDER BY created_at ASC, provider_id ASC",
    )?;
    let rows = stmt.query_map([], |row| row_to_record(row, &active_id))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Read one provider by id. Returns `None` if the row was deleted
/// between a list call and the lookup.
pub fn get(provider_id: &str) -> Result<Option<ProviderRecord>, AiError> {
    let conn = open()?;
    let active_id = read_setting(&conn, ACTIVE_PROVIDER_KEY)?;
    let rec = conn
        .query_row(
            "SELECT provider_id, provider_type, nickname, key_last4, created_at, updated_at
             FROM ai_providers
             WHERE provider_id = ?1",
            params![provider_id],
            |row| row_to_record(row, &active_id),
        )
        .optional()?;
    Ok(rec)
}

/// Add a new provider. Stores the real key in the keychain under
/// `account = provider_id`; records the row with the last-4 and the
/// user-chosen nickname. If this is the user's first provider, it
/// becomes the active provider automatically.
pub fn add(
    provider_type: Provider,
    nickname: String,
    key: Zeroizing<String>,
) -> Result<ProviderRecord, AiError> {
    let nickname = nickname.trim().to_string();
    if nickname.is_empty() || nickname.len() > NICKNAME_MAX_LEN {
        return Err(AiError::InvalidInput("nickname"));
    }
    // Shape-check the key BEFORE we write anything anywhere.
    if !key::looks_like_key(provider_type, key.trim()) {
        return Err(AiError::InvalidInput("ai_api_key"));
    }
    let provider_id = new_provider_id()?;
    let last4 = key_last4(key.trim());
    let now = Utc::now().to_rfc3339();

    // Order matters: write the keychain entry FIRST. If that fails,
    // we've leaked nothing to SQLite. If SQLite then fails, the
    // keychain entry is orphaned — `list()` won't see it, and the
    // next `add()` for the same provider type with a new id will
    // overwrite a different account slot. Acceptable failure mode;
    // the user can re-add.
    key::set_for_id(&provider_id, Zeroizing::new(key.trim().to_string()))?;

    let conn = open()?;
    let active_id = read_setting(&conn, ACTIVE_PROVIDER_KEY)?;
    conn.execute(
        "INSERT INTO ai_providers (provider_id, provider_type, nickname, key_last4,
                                   created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![provider_id, provider_type.as_str(), nickname, last4, now],
    )?;

    // Auto-promote first provider to active so the AI surface stops
    // being dormant the moment the user finishes the Add modal.
    if active_id.is_empty() {
        write_setting(&conn, ACTIVE_PROVIDER_KEY, &provider_id)?;
    }

    // Re-read so `is_active` reflects the just-written state.
    drop(conn);
    Ok(get(&provider_id)?.expect("just-inserted provider row missing"))
}

/// Update a provider's nickname and/or rotate its key. Either field
/// may be `None`; `None` means "don't touch". The provider_id is
/// stable across updates.
pub fn update(
    provider_id: &str,
    new_nickname: Option<String>,
    new_key: Option<Zeroizing<String>>,
) -> Result<ProviderRecord, AiError> {
    let existing = get(provider_id)?.ok_or(AiError::NoProvider)?;

    // Validate inputs before touching any storage.
    let trimmed_nick = new_nickname.as_ref().map(|n| n.trim().to_string());
    if let Some(ref n) = trimmed_nick {
        if n.is_empty() || n.len() > NICKNAME_MAX_LEN {
            return Err(AiError::InvalidInput("nickname"));
        }
    }
    let trimmed_key = new_key.as_ref().map(|k| k.trim().to_string());
    if let Some(ref k) = trimmed_key {
        if !key::looks_like_key(existing.provider_type, k) {
            return Err(AiError::InvalidInput("ai_api_key"));
        }
    }

    // Apply keychain rotation first (same rationale as `add`).
    if let Some(ref k) = trimmed_key {
        key::set_for_id(provider_id, Zeroizing::new(k.clone()))?;
    }

    let conn = open()?;
    let now = Utc::now().to_rfc3339();
    if let (Some(ref n), Some(ref k)) = (&trimmed_nick, &trimmed_key) {
        let last4 = key_last4(k);
        conn.execute(
            "UPDATE ai_providers SET nickname = ?1, key_last4 = ?2, updated_at = ?3
             WHERE provider_id = ?4",
            params![n, last4, now, provider_id],
        )?;
    } else if let Some(ref n) = trimmed_nick {
        conn.execute(
            "UPDATE ai_providers SET nickname = ?1, updated_at = ?2
             WHERE provider_id = ?3",
            params![n, now, provider_id],
        )?;
    } else if let Some(ref k) = trimmed_key {
        let last4 = key_last4(k);
        conn.execute(
            "UPDATE ai_providers SET key_last4 = ?1, updated_at = ?2
             WHERE provider_id = ?3",
            params![last4, now, provider_id],
        )?;
    }
    drop(conn);
    Ok(get(provider_id)?.expect("provider row vanished mid-update"))
}

/// Remove a provider row + its keychain entry. If the row was the
/// active provider, the active id is cleared (the layer goes dormant
/// until the user picks another).
pub fn delete(provider_id: &str) -> Result<(), AiError> {
    let existing = get(provider_id)?.ok_or(AiError::NoProvider)?;
    // Wipe the keychain slot first.
    key::clear_for_id(provider_id)?;
    let conn = open()?;
    conn.execute(
        "DELETE FROM ai_providers WHERE provider_id = ?1",
        params![provider_id],
    )?;
    if existing.is_active {
        write_setting(&conn, ACTIVE_PROVIDER_KEY, "")?;
    }
    Ok(())
}

/// Read the currently-active provider record, if any.
pub fn active() -> Result<Option<ProviderRecord>, AiError> {
    let conn = open()?;
    let active_id = read_setting(&conn, ACTIVE_PROVIDER_KEY)?;
    drop(conn);
    if active_id.is_empty() {
        return Ok(None);
    }
    get(&active_id)
}

/// Choose which provider is active. Passing the empty string clears
/// the selection (the layer becomes dormant).
pub fn set_active(provider_id: &str) -> Result<(), AiError> {
    if !provider_id.is_empty() && get(provider_id)?.is_none() {
        return Err(AiError::NoProvider);
    }
    let conn = open()?;
    write_setting(&conn, ACTIVE_PROVIDER_KEY, provider_id)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_last4_handles_short_and_normal_inputs() {
        assert_eq!(key_last4(""), "");
        assert_eq!(key_last4("abc"), "abc");
        assert_eq!(key_last4("abcd"), "abcd");
        assert_eq!(key_last4("sk-ant-1234567890ABCD"), "ABCD");
    }
}
