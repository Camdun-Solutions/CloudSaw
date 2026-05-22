// Forward-only SQLite migration runner with pre-migration backup.
//
// Contract behavior (see C01-foundation "Edge Cases" + "Constraints"):
//   1. On startup, open (or create) the database at the given path.
//   2. Bootstrap a `_migrations` table tracking which migration ids have run.
//   3. Determine the set of pending migrations (declared in MIGRATIONS).
//   4. If any are pending AND the DB file already existed on disk, copy it to
//      a timestamped backup BEFORE applying them. Backups enable manual
//      recovery — rollback is not supported (CLAUDE.md §6.5).
//   5. Apply pending migrations in a single transaction per migration, and
//      record each in `_migrations`.
//   6. A corrupt/unreadable DB surfaces a typed error rather than silently
//      recreating the file (which would mask data loss).
//
// Migration files live in src-tauri/migrations/ and are embedded into the
// binary via `include_str!`, so the running app never depends on the file
// being shipped alongside it.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use rusqlite::Connection;

use crate::db::paths::set_user_only;
use crate::errors::AppError;

/// Ordered list of declared migrations. Each entry is `(id, sql)`. `id` MUST
/// sort lexicographically in apply-order — the file-name prefix convention
/// (0001, 0002, …) gives this for free.
///
/// To add a migration: drop a new file in src-tauri/migrations/, then add a
/// new line here. The compile step pulls the file content into the binary.
const MIGRATIONS: &[(&str, &str)] = &[
    ("0001_init", include_str!("../../migrations/0001_init.sql")),
    (
        "0002_app_lock",
        include_str!("../../migrations/0002_app_lock.sql"),
    ),
    (
        "0003_accounts",
        include_str!("../../migrations/0003_accounts.sql"),
    ),
    (
        "0004_terraform",
        include_str!("../../migrations/0004_terraform.sql"),
    ),
    (
        "0005_scanner",
        include_str!("../../migrations/0005_scanner.sql"),
    ),
    (
        "0006_findings",
        include_str!("../../migrations/0006_findings.sql"),
    ),
    (
        "0007_scheduler",
        include_str!("../../migrations/0007_scheduler.sql"),
    ),
    (
        "0008_eventlog_retention",
        include_str!("../../migrations/0008_eventlog_retention.sql"),
    ),
];

pub fn run(db_path: &Path) -> Result<(), AppError> {
    apply(db_path, MIGRATIONS)
}

/// Apply an explicit migration set. The production path uses the const
/// `MIGRATIONS`; tests inject custom slices to exercise edge cases (notably
/// the "migration fails mid-run, backup must survive" scenario from the QA
/// contract).
pub fn apply(db_path: &Path, migrations: &[(&str, &str)]) -> Result<(), AppError> {
    let pre_existing = db_path.exists();

    // Open (creates the file if missing). A corrupt file fails here with a
    // typed Db error — we do NOT delete and recreate.
    let mut conn = Connection::open(db_path)
        .map_err(|e| AppError::Db(format!("open {}: {e}", db_path.display())))?;

    // New files inherit broad default ACLs on some platforms; narrow them.
    if !pre_existing {
        set_user_only(db_path, false)?;
    }

    bootstrap_migrations_table(&conn)?;

    let applied: Vec<String> = conn
        .prepare("SELECT id FROM _migrations")?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<_, _>>()?;

    let pending: Vec<&(&str, &str)> = migrations
        .iter()
        .filter(|(id, _)| !applied.iter().any(|a| a == id))
        .collect();

    if pending.is_empty() {
        return Ok(());
    }

    // Pre-migration backup — only meaningful if the DB existed before we
    // opened it. Skip for a brand-new file with nothing in it.
    if pre_existing {
        let backup_path = backup_path_for(db_path);
        fs::copy(db_path, &backup_path).map_err(|e| {
            AppError::Migration(format!(
                "could not write pre-migration backup {}: {e}",
                backup_path.display()
            ))
        })?;
        set_user_only(&backup_path, false)?;
    }

    for (id, sql) in pending {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)
            .map_err(|e| AppError::Migration(format!("{id}: {e}")))?;
        tx.execute(
            "INSERT INTO _migrations (id, applied_at) VALUES (?1, ?2)",
            (*id, Utc::now().to_rfc3339()),
        )?;
        tx.commit()?;
    }

    Ok(())
}

fn bootstrap_migrations_table(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _migrations (
            id          TEXT PRIMARY KEY,
            applied_at  TEXT NOT NULL
        );",
    )?;
    Ok(())
}

fn backup_path_for(db_path: &Path) -> PathBuf {
    let stamp = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let mut name = db_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "cloudsaw.db".into());
    name.push_str(&format!(".pre-migration.{stamp}.bak"));
    db_path.with_file_name(name)
}
