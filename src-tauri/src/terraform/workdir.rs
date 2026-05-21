// Per-account Terraform working directory management.
//
// CLAUDE.md §6.7 places the data root at platform-specific locations with
// `tf-work/{aws_account_id}/` as the per-account Terraform working dir.
// Contract 05 §Constraints adds:
//   * "Terraform state files MUST live only in the per-account working
//     directory — never in the app bundle, never in the repo."
//   * "All Terraform working directories and state files MUST have user-only
//     permissions."
//
// The CloudSaw module source lives at `src-tauri/tf-modules/scanner-role/`
// in the build tree and is mirrored into the working directory before
// `terraform init`. We copy rather than symlink so the working dir stays
// self-contained and survives the bundle being uninstalled/reinstalled.

use std::path::{Path, PathBuf};

use super::error::TerraformError;
use crate::db::paths::{app_data_dir, ensure_user_only_dir, set_user_only};

/// Account-id grammar mirrors the validator in `accounts::validation`:
/// exactly 12 ASCII digits. Defense in depth — we never let an unvalidated
/// account ID become a path segment.
fn validate_account_id(id: &str) -> Result<(), TerraformError> {
    if id.len() != 12 || !id.chars().all(|c| c.is_ascii_digit()) {
        return Err(TerraformError::InvalidInput("aws_account_id"));
    }
    Ok(())
}

/// Absolute path to the per-account working directory. Does NOT create it —
/// `prepare` does that.
pub fn workdir(aws_account_id: &str) -> Result<PathBuf, TerraformError> {
    validate_account_id(aws_account_id)?;
    let root = app_data_dir().map_err(|e| TerraformError::WorkdirIo(e.to_string()))?;
    Ok(root.join("tf-work").join(aws_account_id))
}

/// Ensure the working directory exists, has user-only permissions, and
/// contains a freshly synced copy of the bundled scanner-role module. Safe to
/// call repeatedly — it overlays the latest module source each time so a
/// CloudSaw upgrade that changes the module is picked up on the next plan.
///
/// Returns the absolute working-directory path.
pub fn prepare(aws_account_id: &str) -> Result<PathBuf, TerraformError> {
    let dir = workdir(aws_account_id)?;
    ensure_user_only_dir(&dir).map_err(|e| TerraformError::WorkdirIo(e.to_string()))?;

    // Copy the bundled module source into the workdir. We overwrite on every
    // call so a CloudSaw upgrade picks up the new module. We do NOT delete
    // unknown files in the workdir — Terraform's own `.terraform/` cache and
    // state files live alongside and must be preserved across runs.
    let module_src = locate_module_source()?;
    sync_module(&module_src, &dir)?;

    Ok(dir)
}

/// Locate the read-only `tf-modules/scanner-role/` directory. Lookup order
/// mirrors `binary.rs::locate()`:
///   1. CLOUDSAW_TF_MODULE_OVERRIDE (test seam, absolute path).
///   2. Tauri resource directory (production install).
///   3. exe-relative `tf-modules/scanner-role/`.
///   4. `$CARGO_MANIFEST_DIR/tf-modules/scanner-role/` (dev).
pub fn locate_module_source() -> Result<PathBuf, TerraformError> {
    if let Some(p) = std::env::var_os("CLOUDSAW_TF_MODULE_OVERRIDE") {
        let path = PathBuf::from(p);
        if path.is_dir() {
            return Ok(path);
        }
        return Err(TerraformError::WorkdirIo(format!(
            "CLOUDSAW_TF_MODULE_OVERRIDE points at non-directory: {}",
            path.display()
        )));
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for root in [
                Some(dir.to_path_buf()),
                dir.parent().map(|p| p.join("Resources")),
                dir.parent().map(|p| p.to_path_buf()),
            ]
            .into_iter()
            .flatten()
            {
                let candidate = root.join("tf-modules").join("scanner-role");
                if candidate.is_dir() {
                    return Ok(candidate);
                }
            }
        }
    }

    if let Some(manifest) = std::env::var_os("CARGO_MANIFEST_DIR") {
        let candidate = PathBuf::from(manifest)
            .join("tf-modules")
            .join("scanner-role");
        if candidate.is_dir() {
            return Ok(candidate);
        }
    }

    Err(TerraformError::Internal("module_source_missing"))
}

/// Copy *.tf files from `src` to `dst`, narrowing each file's permissions to
/// user-only. We intentionally do not recurse into subdirectories — the
/// scanner-role module is flat (versions.tf / variables.tf / main.tf /
/// outputs.tf) and a recursive walker would risk picking up unrelated files
/// in an override directory used by a future contract.
fn sync_module(src: &Path, dst: &Path) -> Result<(), TerraformError> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        if !from.is_file() {
            continue;
        }
        let Some(name) = from.file_name() else {
            continue;
        };
        let name_str = name.to_string_lossy();
        let copy = matches!(name_str.as_ref(), "README.md")
            || name_str.ends_with(".tf")
            || name_str.ends_with(".tf.json");
        if !copy {
            continue;
        }
        let to = dst.join(name);
        std::fs::copy(&from, &to)?;
        set_user_only(&to, false).map_err(|e| TerraformError::WorkdirIo(e.to_string()))?;
    }
    Ok(())
}

/// Write the per-plan tfvars JSON into the working directory. The filename
/// `terraform.auto.tfvars.json` is auto-loaded by Terraform on every command,
/// so init/plan/apply all see the same inputs without explicit `-var-file`
/// flags. Permissions are narrowed to user-only — these inputs include the
/// caller's ARN and the per-account external_id, which is configuration not
/// a credential but still kept off the world-readable set.
pub fn write_tfvars(workdir: &Path, body: &str) -> Result<(), TerraformError> {
    let path = workdir.join("terraform.auto.tfvars.json");
    std::fs::write(&path, body)?;
    set_user_only(&path, false).map_err(|e| TerraformError::WorkdirIo(e.to_string()))?;
    Ok(())
}

/// Returns the path Terraform writes the binary plan to. Stored inside the
/// per-account working directory so apply can target the exact plan the user
/// confirmed.
pub fn plan_file(workdir: &Path) -> PathBuf {
    workdir.join("cloudsaw.tfplan")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_malformed_account_id() {
        assert!(matches!(
            validate_account_id("12345"),
            Err(TerraformError::InvalidInput("aws_account_id"))
        ));
        assert!(matches!(
            validate_account_id("12345678901a"),
            Err(TerraformError::InvalidInput("aws_account_id"))
        ));
        assert!(matches!(
            validate_account_id("../etc/passwd"),
            Err(TerraformError::InvalidInput("aws_account_id"))
        ));
        assert!(validate_account_id("111122223333").is_ok());
    }
}
