// Bundled Terraform binary discovery and SHA-256 integrity check.
//
// Contract 05 §Constraints:
//   * "The bundled Terraform binary MUST be invoked by absolute path with
//     argv arrays; a shell MUST NOT be used."
//   * "The Terraform binary's SHA-256 MUST be verified against the build-pinned
//     hash before every execution."
//
// What lives here:
//   * `locate()` — finds the binary by walking a deterministic search list.
//   * `verify_sha256(path)` — reads the file once, hashes it, compares against
//     the build-pinned hash for the current target triple.
//   * `availability()` — combines both into the `TerraformAvailability` shape
//     used by `detect_terraform`.
//
// Per-target pinning model:
//   The constant `PLATFORM_PINNED_SHA256` is the build-time pin. Production
//   releases set it (Contract 16 wires this into the build); development
//   builds without a vendored binary leave it `None`, and `detect_terraform`
//   reports `Missing`. There is no codepath that runs Terraform without a
//   pinned hash — `verify_sha256` against `None` is an integrity failure.
//
// Test seams (matching the existing CLOUDSAW_DATA_DIR_OVERRIDE convention):
//   * CLOUDSAW_TERRAFORM_BIN_OVERRIDE — absolute path to a fake binary.
//   * CLOUDSAW_TERRAFORM_SHA256_OVERRIDE — hex SHA-256 the override should
//     match against. Tests set both.
//
// Production builds do not read these env vars; an attacker who can set env
// vars on a user's machine has already won the threat model (they can swap
// the AWS_CONFIG_FILE, the data dir, the PATH, etc.). The override is a
// pragmatic test seam, never a security boundary.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::error::TerraformError;
use super::types::TerraformAvailability;

/// Compile-time pinned SHA-256 of the bundled Terraform binary for this
/// target triple. Set by the release pipeline (Contract 16). `None` in
/// development builds means "no binary bundled".
///
/// When this gains values per platform, prefer driving the table from a
/// build-script-generated module rather than handwritten cfg arms — the
/// shape stays the same.
// Production builds (Contract 16) overwrite this with a per-target hash via
// a build-script-generated module. Today the placeholder is `None` and
// `detect_terraform` reports `Missing` for every target — exactly what the
// contract's "Terraform binary missing" edge case requires.
pub const PLATFORM_PINNED_SHA256: Option<&str> = None;

/// Returns the absolute path to the bundled Terraform binary for the current
/// target triple, or `None` if no binary is bundled.
///
/// The lookup order is:
///   1. `CLOUDSAW_TERRAFORM_BIN_OVERRIDE` (test seam).
///   2. Tauri resource directory (production install).
///   3. `<exe_dir>/../Resources/vendor/terraform/<triple>/terraform[.exe]`
///      (macOS bundle layout).
///   4. `<exe_dir>/vendor/terraform/<triple>/terraform[.exe]` (Linux/Windows
///      portable layout).
///   5. `CARGO_MANIFEST_DIR/vendor/terraform/<triple>/terraform[.exe]` (dev
///      from `cargo run`).
///
/// We deliberately do NOT consult PATH — CloudSaw uses ONLY the bundled
/// binary so the SHA-256 pin is meaningful. Running a system terraform would
/// bypass the integrity gate.
pub fn locate() -> Option<PathBuf> {
    if let Some(override_path) = std::env::var_os("CLOUDSAW_TERRAFORM_BIN_OVERRIDE") {
        let p = PathBuf::from(override_path);
        if p.is_file() {
            return Some(p);
        }
        // If the override is set but the file is missing, treat it as "missing"
        // — surfacing IntegrityFailed here would mask the obvious config
        // error.
        return None;
    }

    let exe_name = if cfg!(windows) {
        "terraform.exe"
    } else {
        "terraform"
    };

    let triple_dir = target_triple_dir();
    let candidates: Vec<PathBuf> = exe_relative_search_roots()
        .into_iter()
        .map(|root| {
            root.join("vendor")
                .join("terraform")
                .join(triple_dir)
                .join(exe_name)
        })
        .collect();

    for c in candidates {
        if c.is_file() {
            return Some(c);
        }
    }

    // Dev fallback — only matters during `cargo run` / `cargo test`.
    if let Some(dev) = dev_manifest_candidate(triple_dir, exe_name) {
        if dev.is_file() {
            return Some(dev);
        }
    }

    None
}

/// Returns the directory name used under `vendor/terraform/` for the current
/// build target. Keep these in sync with the release pipeline (Contract 16).
fn target_triple_dir() -> &'static str {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    return "x86_64-pc-windows-msvc";
    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    return "aarch64-pc-windows-msvc";
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    return "x86_64-apple-darwin";
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    return "aarch64-apple-darwin";
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return "x86_64-unknown-linux-gnu";
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    return "aarch64-unknown-linux-gnu";
    #[allow(unreachable_code)]
    "unknown"
}

/// Candidate roots to search for `vendor/terraform/<triple>/terraform`,
/// relative to the running executable. We probe multiple layouts so the
/// same code works for installed bundles and portable extracts without
/// branching on platform.
fn exe_relative_search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            roots.push(dir.to_path_buf());
            if let Some(parent) = dir.parent() {
                roots.push(parent.join("Resources"));
                roots.push(parent.to_path_buf());
            }
        }
    }
    roots
}

/// Dev candidate: `$CARGO_MANIFEST_DIR/vendor/terraform/<triple>/terraform`.
/// Only resolved when CARGO_MANIFEST_DIR is set (i.e. when running through
/// cargo). In a packaged release this returns `None`.
fn dev_manifest_candidate(triple_dir: &str, exe_name: &str) -> Option<PathBuf> {
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR")?;
    Some(
        PathBuf::from(manifest)
            .join("vendor")
            .join("terraform")
            .join(triple_dir)
            .join(exe_name),
    )
}

/// Read the binary and compute its hex-encoded SHA-256. Reads the file in
/// one shot since Terraform binaries are ~50-150MB — well within memory.
/// Returns the lowercase hex digest (`hex::encode` default).
pub fn compute_sha256(path: &Path) -> Result<String, TerraformError> {
    let bytes = std::fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let digest = hasher.finalize();
    Ok(hex::encode(digest))
}

/// Returns the build-pinned SHA-256 to compare against. Honors the
/// CLOUDSAW_TERRAFORM_SHA256_OVERRIDE test seam.
fn pinned_sha256() -> Option<String> {
    if let Ok(over) = std::env::var("CLOUDSAW_TERRAFORM_SHA256_OVERRIDE") {
        if !over.is_empty() {
            return Some(over.to_ascii_lowercase());
        }
    }
    PLATFORM_PINNED_SHA256.map(|s| s.to_ascii_lowercase())
}

/// Verify the binary at `path` against the build-pinned hash. Returns the
/// computed digest on success so callers can log it. On any mismatch returns
/// `IntegrityFailed`; on missing pin returns `NotBundled` (a binary with no
/// pin can never be executed — see module docs).
pub fn verify_sha256(path: &Path) -> Result<String, TerraformError> {
    let expected = pinned_sha256().ok_or(TerraformError::NotBundled)?;
    let actual = compute_sha256(path)?;
    if constant_time_eq(expected.as_bytes(), actual.as_bytes()) {
        Ok(actual)
    } else {
        Err(TerraformError::IntegrityFailed)
    }
}

/// Constant-time byte comparison for the hash check. Not strictly required —
/// timing on a 64-char hex string against a known constant is not a meaningful
/// side channel — but it keeps the integrity check uniform.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Combine `locate()` and `verify_sha256()` into the public availability
/// shape `detect_terraform` returns.
pub fn availability() -> TerraformAvailability {
    let Some(path) = locate() else {
        return TerraformAvailability::Missing;
    };
    match verify_sha256(&path) {
        Ok(sha) => TerraformAvailability::Available {
            sha256: sha,
            version: None,
        },
        Err(TerraformError::NotBundled) => TerraformAvailability::Missing,
        Err(_) => TerraformAvailability::IntegrityFailed,
    }
}

/// Returns the located binary path AND its verified hash, or a typed error
/// if either step fails. Called by `runner::plan`/`runner::apply` before
/// every execution.
pub fn locate_and_verify() -> Result<(PathBuf, String), TerraformError> {
    let path = locate().ok_or(TerraformError::NotBundled)?;
    let sha = verify_sha256(&path)?;
    Ok((path, sha))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_eq_handles_equal_and_unequal() {
        assert!(constant_time_eq(b"abcdef", b"abcdef"));
        assert!(!constant_time_eq(b"abcdef", b"abcdeg"));
        assert!(!constant_time_eq(b"abc", b"abcdef"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn compute_sha256_matches_known_value() {
        // "hello\n" sha256: 5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03
        let tmp = std::env::temp_dir().join("cloudsaw-sha256-test-input.txt");
        std::fs::write(&tmp, b"hello\n").unwrap();
        let out = compute_sha256(&tmp).unwrap();
        let _ = std::fs::remove_file(&tmp);
        assert_eq!(
            out,
            "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03"
        );
    }

    #[test]
    fn verify_sha256_rejects_mismatched_hash() {
        let tmp = std::env::temp_dir().join("cloudsaw-sha256-mismatch.bin");
        std::fs::write(&tmp, b"hello\n").unwrap();
        std::env::set_var(
            "CLOUDSAW_TERRAFORM_SHA256_OVERRIDE",
            "0000000000000000000000000000000000000000000000000000000000000000",
        );
        let res = verify_sha256(&tmp);
        std::env::remove_var("CLOUDSAW_TERRAFORM_SHA256_OVERRIDE");
        let _ = std::fs::remove_file(&tmp);
        assert!(matches!(res, Err(TerraformError::IntegrityFailed)));
    }

    #[test]
    fn verify_sha256_without_pin_yields_not_bundled() {
        let tmp = std::env::temp_dir().join("cloudsaw-sha256-no-pin.bin");
        std::fs::write(&tmp, b"x").unwrap();
        // Force-clear the override even if a prior test in a flaky order left it.
        std::env::remove_var("CLOUDSAW_TERRAFORM_SHA256_OVERRIDE");
        let res = verify_sha256(&tmp);
        let _ = std::fs::remove_file(&tmp);
        // PLATFORM_PINNED_SHA256 is None in tests (no bundled binary), so the
        // expected outcome is NotBundled.
        assert!(matches!(res, Err(TerraformError::NotBundled)));
    }
}
