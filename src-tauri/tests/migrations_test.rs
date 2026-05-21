// Integration tests for the migration runner. These exercise the *real*
// migration list compiled into the library — there is no mocking — because
// the migration runner is the single mechanism that touches user data, and
// regressions here can cause silent data loss.
//
// Each test uses a fresh tempdir so they're hermetic and parallel-safe.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

use cloudsaw_lib::db::migrations;
use cloudsaw_lib::errors::AppError;

fn fresh_tempdir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("cloudsaw-test-{label}-{nanos}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn fresh_database_applies_initial_migration() {
    let dir = fresh_tempdir("fresh");
    let db = dir.join("cloudsaw.db");

    migrations::run(&db).expect("migration runner should succeed on fresh DB");

    let conn = Connection::open(&db).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM _migrations WHERE id = '0001_init'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "0001_init should be recorded as applied");

    // The migration created the settings table and seeded a single row.
    let seeded: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM settings WHERE key = 'schema_initialized'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(seeded, 1);
}

#[test]
fn rerun_is_idempotent_and_writes_no_backup() {
    let dir = fresh_tempdir("idempotent");
    let db = dir.join("cloudsaw.db");

    migrations::run(&db).unwrap();

    let backups_before = count_backup_files(&dir);
    migrations::run(&db).unwrap();
    let backups_after = count_backup_files(&dir);

    assert_eq!(
        backups_before, backups_after,
        "re-running with no pending migrations must not create a backup"
    );
}

#[test]
fn pending_migration_on_existing_database_writes_pre_migration_backup() {
    // Simulate the "DB exists at older schema" edge case from C01:
    // create a DB file that lacks _migrations, then run the migrator and
    // assert a backup file is produced before the schema is changed.
    let dir = fresh_tempdir("backup");
    let db = dir.join("cloudsaw.db");
    {
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch("CREATE TABLE legacy (id INTEGER PRIMARY KEY);")
            .unwrap();
    }

    let backups_before = count_backup_files(&dir);
    migrations::run(&db).unwrap();
    let backups_after = count_backup_files(&dir);

    assert_eq!(
        backups_after,
        backups_before + 1,
        "exactly one backup file should be produced for a real upgrade"
    );

    // The legacy table is preserved (forward-only migrations don't drop
    // pre-existing tables).
    let conn = Connection::open(&db).unwrap();
    let legacy_present: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='legacy'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(legacy_present, 1);
}

#[test]
fn corrupt_database_surfaces_typed_error_without_recreating() {
    let dir = fresh_tempdir("corrupt");
    let db = dir.join("cloudsaw.db");
    // Write a file that is decisively not a SQLite database.
    fs::write(&db, b"\x00not-a-sqlite-file\x00garbage\x00bytes\x00").unwrap();

    let original_bytes = fs::read(&db).unwrap();

    let result = migrations::run(&db);
    assert!(
        matches!(result, Err(AppError::Db(_)) | Err(AppError::Migration(_))),
        "corrupt DB must produce a typed AppError, got {result:?}"
    );

    let after_bytes = fs::read(&db).unwrap();
    assert_eq!(
        original_bytes, after_bytes,
        "the migrator must NOT silently recreate or overwrite a corrupt DB"
    );
}

#[test]
fn failing_migration_aborts_run_and_keeps_pre_migration_backup_intact() {
    // QA error-state: "Migration failure mid-run → app reports the failure
    // and the pre-migration backup exists intact." We seed the DB at an
    // earlier schema, then ask `apply` to run two migrations where the
    // second is malformed SQL. The runner should return AppError::Migration,
    // _migrations should NOT record the failing id, and the backup written
    // before the run must still exist with the original DB contents.
    let dir = fresh_tempdir("midrun");
    let db = dir.join("cloudsaw.db");
    {
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch("CREATE TABLE legacy (id INTEGER PRIMARY KEY);")
            .unwrap();
        conn.execute("INSERT INTO legacy (id) VALUES (42);", [])
            .unwrap();
    }

    let custom_migrations: &[(&str, &str)] = &[
        ("9001_ok", "CREATE TABLE step_one (id INTEGER PRIMARY KEY);"),
        ("9002_bad", "THIS IS NOT VALID SQL;"),
    ];

    let result = migrations::apply(&db, custom_migrations);
    assert!(
        matches!(result, Err(AppError::Migration(_))),
        "malformed migration should surface AppError::Migration, got {result:?}"
    );

    // A backup file must exist after the failed run, and the user's data
    // (legacy row id=42) must be readable from it. The backup also must
    // NOT yet contain the half-applied state from inside the run — the
    // user can restore to it and end up exactly where they started.
    let backups = backup_files(&dir);
    assert_eq!(
        backups.len(),
        1,
        "exactly one pre-migration backup should exist after a failed run"
    );
    let backup_conn = Connection::open(&backups[0]).unwrap();
    let legacy_rows: i64 = backup_conn
        .query_row("SELECT COUNT(*) FROM legacy WHERE id = 42", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(
        legacy_rows, 1,
        "backup must preserve the user's pre-migration data (legacy row id=42)"
    );
    let step_one_in_backup: i64 = backup_conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='step_one'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        step_one_in_backup, 0,
        "backup must predate any migration being applied (step_one MUST NOT be in it)"
    );

    // The failing migration's transaction rolled back, so 9002_bad must not
    // appear in _migrations.
    let conn = Connection::open(&db).unwrap();
    let bad_recorded: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM _migrations WHERE id = '9002_bad'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        bad_recorded, 0,
        "failing migration must not be recorded as applied"
    );
}

fn backup_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().contains(".pre-migration."))
        .map(|e| e.path())
        .collect()
}

fn count_backup_files(dir: &std::path::Path) -> usize {
    fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().contains(".pre-migration."))
        .count()
}
