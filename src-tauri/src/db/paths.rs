// Per-platform app data directory + user-only permission enforcement.
// See CLAUDE.md §6.7 for the layout and §4.5 for file-permission rules.

use std::path::{Path, PathBuf};

use crate::errors::AppError;

/// Resolves the CloudSaw app data root.
///   Windows: %APPDATA%\CloudSaw\
///   macOS:   ~/Library/Application Support/CloudSaw/
///   Linux:   ~/.local/share/cloudsaw/
///
/// Test seam: setting `CLOUDSAW_DATA_DIR_OVERRIDE` to an absolute path makes
/// every consumer (migrations, app-lock storage) use that path instead. The
/// override is read on every call so integration tests can swap directories
/// between tests. We don't gate on `#[cfg(test)]` because integration tests
/// link against the release-shape library — the override is always honored
/// but unset in production runs.
pub fn app_data_dir() -> Result<PathBuf, AppError> {
    if let Some(override_path) = std::env::var_os("CLOUDSAW_DATA_DIR_OVERRIDE") {
        return Ok(PathBuf::from(override_path));
    }
    let base =
        dirs::data_dir().ok_or_else(|| AppError::Path("could not resolve user data dir".into()))?;
    let name = if cfg!(target_os = "linux") {
        "cloudsaw"
    } else {
        "CloudSaw"
    };
    Ok(base.join(name))
}

/// Creates `dir` (and parents) if missing, then narrows permissions so only
/// the current user can read/write it. On Unix this is mode 0700; on Windows
/// the directory inherits user-profile ACLs by virtue of living under
/// %APPDATA% (which is already user-restricted).
pub fn ensure_user_only_dir(dir: &Path) -> Result<(), AppError> {
    std::fs::create_dir_all(dir)?;
    set_user_only(dir, true)?;
    Ok(())
}

/// Narrows permissions on an existing path. `is_dir = true` uses 0700;
/// otherwise 0600.
pub fn set_user_only(path: &Path, is_dir: bool) -> Result<(), AppError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if is_dir { 0o700 } else { 0o600 };
        let perms = std::fs::Permissions::from_mode(mode);
        std::fs::set_permissions(path, perms)?;
    }
    #[cfg(windows)]
    {
        // %APPDATA% lives under the user profile, which already has a
        // user-only ACL by default. We don't tighten further here; the data
        // root contract (CLAUDE.md §6.7) sits inside that protected tree.
        // Explicit ACL hardening is reserved for files containing scan data
        // and is wired up in the contracts that produce them.
        let _ = (path, is_dir);
    }
    Ok(())
}
