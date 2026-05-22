// SQLite read/write for the github integration. Two surfaces:
//
//   * The findings-ticket destination repo (a row in the existing
//     `settings` table — non-secret).
//   * The `finding_tickets` table — one row per (finding_id, issue)
//     link, used by the UI to render "this finding is tracked in #N".
//
// CLAUDE.md §4.5: every statement uses parameterized binds.

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};

use super::error::GithubError;
use super::types::{FindingTicket, RepoSelection};
use crate::db::paths::app_data_dir;

const KEY_FINDINGS_REPO: &str = "github_findings_repo";

fn db_path() -> Result<std::path::PathBuf, GithubError> {
    Ok(app_data_dir()
        .map_err(|e| GithubError::Io(e.to_string()))?
        .join("db")
        .join("cloudsaw.db"))
}

fn open() -> Result<Connection, GithubError> {
    Connection::open(db_path()?).map_err(GithubError::from)
}

pub fn get_findings_repo() -> Result<Option<RepoSelection>, GithubError> {
    let conn = open()?;
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![KEY_FINDINGS_REPO],
            |r| r.get(0),
        )
        .optional()?;
    match raw {
        None => Ok(None),
        Some(s) if s.trim().is_empty() => Ok(None),
        Some(s) => parse_repo(&s).map(Some),
    }
}

pub fn set_findings_repo(repo: Option<&RepoSelection>) -> Result<(), GithubError> {
    let value = match repo {
        Some(r) => {
            validate_repo(r)?;
            r.as_path()
        }
        None => String::new(),
    };
    let conn = open()?;
    conn.execute(
        "INSERT INTO settings (key, value, updated_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value,
                                        updated_at = excluded.updated_at",
        params![KEY_FINDINGS_REPO, value, Utc::now().to_rfc3339()],
    )?;
    Ok(())
}

pub fn upsert_finding_ticket(
    finding_id: &str,
    aws_account_id: &str,
    repo: &RepoSelection,
    issue_number: u32,
    issue_url: &str,
) -> Result<FindingTicket, GithubError> {
    if !is_valid_finding_id(finding_id) {
        return Err(GithubError::InvalidInput("finding_id"));
    }
    validate_repo(repo)?;
    let conn = open()?;
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO finding_tickets (
            finding_id, aws_account_id, repo_owner, repo_name,
            issue_number, issue_url, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(finding_id) DO UPDATE SET
            repo_owner = excluded.repo_owner,
            repo_name  = excluded.repo_name,
            issue_number = excluded.issue_number,
            issue_url    = excluded.issue_url,
            created_at   = excluded.created_at",
        params![
            finding_id,
            aws_account_id,
            repo.owner,
            repo.name,
            issue_number as i64,
            issue_url,
            now,
        ],
    )?;
    get_finding_ticket(finding_id)
        .transpose()
        .unwrap_or(Err(GithubError::Db("ticket lost after upsert".into())))
}

pub fn get_finding_ticket(finding_id: &str) -> Result<Option<FindingTicket>, GithubError> {
    if !is_valid_finding_id(finding_id) {
        return Err(GithubError::InvalidInput("finding_id"));
    }
    let conn = open()?;
    let row = conn
        .query_row(
            "SELECT finding_id, aws_account_id, repo_owner, repo_name,
                    issue_number, issue_url, created_at
               FROM finding_tickets
              WHERE finding_id = ?1",
            params![finding_id],
            row_to_ticket,
        )
        .optional()?;
    match row {
        None => Ok(None),
        Some(r) => Ok(Some(r?)),
    }
}

pub fn list_tickets_for_account(
    aws_account_id: &str,
    limit: i64,
) -> Result<Vec<FindingTicket>, GithubError> {
    let conn = open()?;
    let bounded = limit.clamp(1, 500);
    let mut stmt = conn.prepare(
        "SELECT finding_id, aws_account_id, repo_owner, repo_name,
                issue_number, issue_url, created_at
           FROM finding_tickets
          WHERE aws_account_id = ?1
          ORDER BY created_at DESC
          LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![aws_account_id, bounded], row_to_ticket)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r??);
    }
    Ok(out)
}

pub fn validate_repo(repo: &RepoSelection) -> Result<(), GithubError> {
    if !is_valid_segment(&repo.owner) || !is_valid_segment(&repo.name) {
        return Err(GithubError::InvalidInput("repo"));
    }
    Ok(())
}

fn parse_repo(raw: &str) -> Result<RepoSelection, GithubError> {
    let parts: Vec<&str> = raw.splitn(2, '/').collect();
    if parts.len() != 2 {
        return Err(GithubError::InvalidInput("repo"));
    }
    let repo = RepoSelection {
        owner: parts[0].to_string(),
        name: parts[1].to_string(),
    };
    validate_repo(&repo)?;
    Ok(repo)
}

fn is_valid_segment(s: &str) -> bool {
    if s.is_empty() || s.len() > 39 {
        return false;
    }
    // GitHub usernames/orgs: alphanumeric or hyphen, can't start/end with hyphen.
    // Repo names: alphanumeric, hyphen, underscore, period.
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        && !s.starts_with('-')
        && !s.ends_with('-')
}

fn is_valid_finding_id(id: &str) -> bool {
    id.len() == 64 && id.chars().all(|c| c.is_ascii_hexdigit())
}

fn row_to_ticket(row: &rusqlite::Row<'_>) -> rusqlite::Result<Result<FindingTicket, GithubError>> {
    let finding_id: String = row.get(0)?;
    let aws_account_id: String = row.get(1)?;
    let repo_owner: String = row.get(2)?;
    let repo_name: String = row.get(3)?;
    let issue_number: i64 = row.get(4)?;
    let issue_url: String = row.get(5)?;
    let created_at_raw: String = row.get(6)?;

    let created_at: DateTime<Utc> = match DateTime::parse_from_rfc3339(&created_at_raw) {
        Ok(dt) => dt.with_timezone(&Utc),
        Err(_) => return Ok(Err(GithubError::Db("bad ticket created_at".into()))),
    };

    Ok(Ok(FindingTicket {
        finding_id,
        aws_account_id_masked: crate::accounts::mask_for_logs(&aws_account_id),
        repo: RepoSelection {
            owner: repo_owner,
            name: repo_name,
        },
        issue_number: issue_number as u32,
        issue_url,
        created_at,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_validation_matches_github_rules() {
        assert!(is_valid_segment("Camdun-Solutions"));
        assert!(is_valid_segment("cloud-saw"));
        assert!(is_valid_segment("my.repo_v1"));
        assert!(!is_valid_segment(""));
        assert!(!is_valid_segment("-leading-hyphen"));
        assert!(!is_valid_segment("trailing-hyphen-"));
        assert!(!is_valid_segment("path/traversal"));
        assert!(!is_valid_segment("contains space"));
    }

    #[test]
    fn finding_id_validation_requires_64_hex() {
        let good = "a".repeat(64);
        assert!(is_valid_finding_id(&good));
        assert!(!is_valid_finding_id(&"a".repeat(63)));
        assert!(!is_valid_finding_id("xyz"));
    }

    #[test]
    fn parse_repo_rejects_missing_slash() {
        assert!(parse_repo("acme").is_err());
        assert!(parse_repo("acme/repo").is_ok());
    }
}
