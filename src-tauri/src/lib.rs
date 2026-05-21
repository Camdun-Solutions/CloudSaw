// CloudSaw library crate. Hosts every privileged module and the Tauri runtime
// setup. The `run()` function is the single entry point invoked from `main.rs`
// (desktop) and from platform-specific entry points (mobile, future).

pub mod auth;
pub mod db;
pub mod errors;
pub mod findings;
pub mod ipc;
pub mod knowledgebase;
pub mod reports;
pub mod scanner;
pub mod terraform;

use crate::errors::AppError;

/// Application entry point. Initializes app data dirs, runs SQLite migrations,
/// registers IPC commands, and launches the Tauri runtime.
pub fn run() {
    if let Err(err) = bootstrap() {
        // Bootstrap failures happen before the UI exists, so we can't surface a
        // stable error code through IPC. Log to stderr and exit non-zero so the
        // OS / CI / user sees the problem rather than a silent crash.
        eprintln!("cloudsaw bootstrap failed: {err}");
        std::process::exit(1);
    }

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![ipc::app_version,])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn bootstrap() -> Result<(), AppError> {
    let data_root = db::paths::app_data_dir()?;
    db::paths::ensure_user_only_dir(&data_root)?;

    let db_dir = data_root.join("db");
    db::paths::ensure_user_only_dir(&db_dir)?;

    let db_path = db_dir.join("cloudsaw.db");
    db::migrations::run(&db_path)?;

    Ok(())
}
