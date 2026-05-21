// IPC surface. Every `#[tauri::command]` declared here MUST:
//   - validate its inputs (no command trusts a value from the frontend)
//   - return `Result<T, AppError>` (or an infallible primitive)
//   - never accept or return credential-bearing types
//
// IPC payloads use plain serializable structs. AWS SDK types never cross this
// boundary. See CLAUDE.md §4.1.

/// Returns the running CalVer build string (e.g. "2026.5.0").
///
/// Trivially derived from `CARGO_PKG_VERSION` at compile time. Exposed so the
/// UI can render "About" / update-banner copy from one source of truth.
#[tauri::command]
pub fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}
