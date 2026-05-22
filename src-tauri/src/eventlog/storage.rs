// SQLite-backed read/write for the `event_log` table.
//
// CLAUDE.md §4.5: every statement uses parameterized binds. Append-only is
// enforced here by the fact that no `UPDATE event_log` or `DELETE FROM
// event_log` runs from anywhere except (a) the retention sweep in
// `eventlog::retention_purge` and (b) the panic wipe in `wipe::run_panic`.
// User-driven UI calls only insert.

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};

use super::error::EventLogError;
use super::types::{EventInput, EventKind, EventLogEntry, EventLogFilter};
use crate::db::paths::app_data_dir;

fn db_path() -> Result<std::path::PathBuf, EventLogError> {
    Ok(app_data_dir()
        .map_err(|e| EventLogError::Io(e.to_string()))?
        .join("db")
        .join("cloudsaw.db"))
}

fn open() -> Result<Connection, EventLogError> {
    Connection::open(db_path()?).map_err(EventLogError::from)
}

/// Insert one row. Caller has already validated user-supplied strings and
/// stripped any secret-bearing fields.
pub fn insert(event_id: &str, occurred_at: DateTime<Utc>, input: &EventInput) -> Result<(), EventLogError> {
    let conn = open()?;
    conn.execute(
        "INSERT INTO event_log (
            event_id, occurred_at, kind, summary, detail,
            aws_account_id, scan_id, path, item_count
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            event_id,
            occurred_at.to_rfc3339(),
            input.kind.as_str(),
            input.summary,
            input.detail,
            input.aws_account_id,
            input.scan_id,
            input.path,
            input.item_count,
        ],
    )?;
    Ok(())
}

/// Read events matching `filter`. The `include_cleared` flag bypasses the
/// `event_log_view.cleared_at` marker — Export uses that path so the
/// exported file always represents the underlying log, not the filtered
/// view.
pub fn list(filter: &EventLogFilter) -> Result<Vec<EventLogEntry>, EventLogError> {
    let conn = open()?;

    let cleared_at = if filter.include_cleared {
        None
    } else {
        get_cleared_at_with(&conn)?
    };

    let mut sql = String::from(
        "SELECT event_id, occurred_at, kind, summary, detail,
                aws_account_id, scan_id, path, item_count
           FROM event_log
          WHERE 1=1",
    );
    let mut binds: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if !filter.kinds.is_empty() {
        let placeholders: Vec<String> = filter
            .kinds
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", binds.len() + i + 1))
            .collect();
        sql.push_str(&format!(" AND kind IN ({})", placeholders.join(", ")));
        for k in &filter.kinds {
            binds.push(Box::new(k.as_str().to_string()));
        }
    }

    if let Some(since) = filter.since {
        binds.push(Box::new(since.to_rfc3339()));
        sql.push_str(&format!(" AND occurred_at >= ?{}", binds.len()));
    }
    if let Some(until) = filter.until {
        binds.push(Box::new(until.to_rfc3339()));
        sql.push_str(&format!(" AND occurred_at <= ?{}", binds.len()));
    }
    if let Some(cleared) = cleared_at {
        binds.push(Box::new(cleared.to_rfc3339()));
        sql.push_str(&format!(" AND occurred_at >= ?{}", binds.len()));
    }

    sql.push_str(" ORDER BY occurred_at DESC, event_id DESC");

    let limit = filter.limit.unwrap_or(500).clamp(1, 5000);
    let offset = filter.offset.unwrap_or(0).max(0);
    binds.push(Box::new(limit));
    sql.push_str(&format!(" LIMIT ?{}", binds.len()));
    binds.push(Box::new(offset));
    sql.push_str(&format!(" OFFSET ?{}", binds.len()));

    let mut stmt = conn.prepare(&sql)?;
    let bind_refs: Vec<&dyn rusqlite::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
    let rows = stmt.query_map(bind_refs.as_slice(), row_to_entry)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r??);
    }
    Ok(out)
}

/// Substring search across the `summary` and `detail` columns. Results are
/// already redacted (the row's `aws_account_id` is masked in `row_to_entry`).
pub fn search(query: &str, limit: i64) -> Result<Vec<EventLogEntry>, EventLogError> {
    let conn = open()?;
    let cleared_at = get_cleared_at_with(&conn)?;
    let bounded_limit = limit.clamp(1, 5000);
    let needle = format!("%{}%", escape_like(query));
    let mut stmt = conn.prepare(
        "SELECT event_id, occurred_at, kind, summary, detail,
                aws_account_id, scan_id, path, item_count
           FROM event_log
          WHERE (summary LIKE ?1 ESCAPE '\\'
                 OR COALESCE(detail, '') LIKE ?1 ESCAPE '\\')
            AND (?2 IS NULL OR occurred_at >= ?2)
          ORDER BY occurred_at DESC, event_id DESC
          LIMIT ?3",
    )?;
    let cleared_str = cleared_at.map(|c| c.to_rfc3339());
    let rows = stmt.query_map(params![needle, cleared_str, bounded_limit], row_to_entry)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r??);
    }
    Ok(out)
}

/// LIKE has two wildcards: `%` and `_`. Escape both so user input is
/// treated literally; the query above declares `ESCAPE '\\'`.
fn escape_like(input: &str) -> String {
    let mut s = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '%' | '_' | '\\' => {
                s.push('\\');
                s.push(c);
            }
            other => s.push(other),
        }
    }
    s
}

/// Read every row in occurred-at order, for export. Bypasses the cleared
/// marker — exports always represent the underlying log.
pub fn list_all_for_export() -> Result<Vec<EventLogEntry>, EventLogError> {
    let conn = open()?;
    let mut stmt = conn.prepare(
        "SELECT event_id, occurred_at, kind, summary, detail,
                aws_account_id, scan_id, path, item_count
           FROM event_log
          ORDER BY occurred_at ASC, event_id ASC",
    )?;
    let rows = stmt.query_map([], row_to_entry)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r??);
    }
    Ok(out)
}

/// Set the "cleared at" marker to `at`. Pass `Utc::now()` to hide every
/// entry written before this call from the default list. Underlying
/// rows are never modified — they still age out via retention and still
/// surface in Export.
pub fn set_cleared_at(at: DateTime<Utc>) -> Result<(), EventLogError> {
    let conn = open()?;
    conn.execute(
        "UPDATE event_log_view SET cleared_at = ?1 WHERE id = 1",
        params![at.to_rfc3339()],
    )?;
    Ok(())
}

/// Read the cleared marker. Returns Ok(None) when the user has never
/// invoked "Clear all".
pub fn get_cleared_at() -> Result<Option<DateTime<Utc>>, EventLogError> {
    let conn = open()?;
    get_cleared_at_with(&conn)
}

fn get_cleared_at_with(conn: &Connection) -> Result<Option<DateTime<Utc>>, EventLogError> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT cleared_at FROM event_log_view WHERE id = 1",
            [],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten();
    match raw {
        None => Ok(None),
        Some(s) if s.is_empty() => Ok(None),
        Some(s) => Ok(Some(
            DateTime::parse_from_rfc3339(&s)
                .map_err(|e| EventLogError::Db(format!("bad cleared_at: {e}")))?
                .with_timezone(&Utc),
        )),
    }
}

/// Delete every event_log row strictly older than `cutoff`. Returns the
/// number of rows actually removed.
pub fn purge_older_than(cutoff: DateTime<Utc>) -> Result<usize, EventLogError> {
    let conn = open()?;
    let affected = conn.execute(
        "DELETE FROM event_log WHERE occurred_at < ?1",
        params![cutoff.to_rfc3339()],
    )?;
    Ok(affected as usize)
}

/// Total row count. Used by the QA contract's responsiveness check.
pub fn count() -> Result<i64, EventLogError> {
    let conn = open()?;
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM event_log", [], |r| r.get(0))?;
    Ok(n)
}

/// Hard-wipe every row in the table. Used only by the panic action. The
/// `cleared_at` marker is reset to NULL so a future log starts fresh.
pub fn wipe_all() -> Result<(), EventLogError> {
    let conn = open()?;
    conn.execute("DELETE FROM event_log", [])?;
    conn.execute("UPDATE event_log_view SET cleared_at = NULL WHERE id = 1", [])?;
    Ok(())
}

fn row_to_entry(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<Result<EventLogEntry, EventLogError>> {
    let event_id: String = row.get(0)?;
    let occurred_at_raw: String = row.get(1)?;
    let kind_raw: String = row.get(2)?;
    let summary: String = row.get(3)?;
    let detail: Option<String> = row.get(4)?;
    let aws_account_id: Option<String> = row.get(5)?;
    let scan_id: Option<String> = row.get(6)?;
    let path: Option<String> = row.get(7)?;
    let item_count: Option<i64> = row.get(8)?;

    let occurred_at = DateTime::parse_from_rfc3339(&occurred_at_raw)
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(e),
            )
        })?
        .with_timezone(&Utc);

    let kind = match EventKind::from_storage(&kind_raw) {
        Some(k) => k,
        None => return Ok(Err(EventLogError::Db(format!("bad kind in row: {kind_raw}")))),
    };

    let masked = aws_account_id.as_deref().map(crate::accounts::mask_for_logs);

    Ok(Ok(EventLogEntry {
        event_id,
        occurred_at,
        kind,
        summary,
        detail,
        aws_account_id_masked: masked,
        scan_id,
        path,
        item_count,
    }))
}
