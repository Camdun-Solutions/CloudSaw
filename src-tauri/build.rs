// Tauri runtime setup + Contract 08 §Constraints build-time check:
// "A markdown file exceeding a sane size limit MUST raise a build-time
// warning." We walk knowledgebase/articles/ and emit cargo::warning for
// any file over the limit so a maintainer notices at `cargo build` time
// rather than during a runtime scan.
//
// Also generates `${OUT_DIR}/scoutsuite_pin.rs` (Phase 1 — ScoutSuite
// bundling) — Rust source that declares the compile-time
// `PLATFORM_PINNED_SHA256` constant the runtime integrity check at
// `src/scanner/binary.rs:37` consumes. The constant comes from
// `vendor/scoutsuite/pinned.json`, which the release workflow writes
// after staging the per-platform PyInstaller-frozen binary. When the
// pinned.json file is absent (dev builds), the generated file declares
// `PLATFORM_PINNED_SHA256 = None` so the existing "missing" UX path
// stays intact.

use std::path::{Path, PathBuf};

const KB_ARTICLE_MAX_BYTES: u64 = 64 * 1024;

fn main() {
    warn_oversized_kb_articles();
    generate_scoutsuite_pin();
    ensure_scoutsuite_bundle_target();
    generate_logo_base64();
    tauri_build::build();
}

/// Generate `${OUT_DIR}/logo_base64.rs` containing the 128px CloudSaw
/// logo as a const base64 string. Embedded into the exported HTML
/// report header by `src/reports/html.rs::render_header()` so the
/// report renders the brand without any external file references —
/// matches the existing "self-contained, no remote URLs" posture
/// already enforced for CSS and other static assets there.
///
/// We read from `icons/128x128.png` (Tauri-generated from the user's
/// source artwork) rather than embedding the 1MB 1024x1024 source
/// directly; 128px is enough for the report header and keeps the
/// compiled binary small.
fn generate_logo_base64() {
    let logo_path = Path::new("icons").join("128x128.png");
    println!("cargo:rerun-if-changed={}", logo_path.display());

    let out_dir = match std::env::var_os("OUT_DIR") {
        Some(d) => PathBuf::from(d),
        None => return,
    };
    let out_file = out_dir.join("logo_base64.rs");

    let body = match std::fs::read(&logo_path) {
        Ok(bytes) => {
            let encoded = base64_encode(&bytes);
            format!("pub const LOGO_PNG_BASE64: &str = \"{encoded}\";\n")
        }
        Err(_) => {
            // Logo missing during dev (icons not regenerated). Empty
            // string makes the HTML report fall back to no logo;
            // existing CSS hides the <img> on `src=""`.
            "pub const LOGO_PNG_BASE64: &str = \"\";\n".to_string()
        }
    };

    if let Err(e) = std::fs::write(&out_file, body) {
        panic!("failed to write {}: {e}", out_file.display());
    }
}

/// Minimal RFC 4648 base64 encoder. Inline so we don't introduce a
/// build-time dep just to encode one PNG. Standard alphabet + padding.
fn base64_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    let mut i = 0;
    while i + 3 <= input.len() {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8) | (input[i + 2] as u32);
        out.push(ALPHABET[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3F) as usize] as char);
        out.push(ALPHABET[((n >> 6) & 0x3F) as usize] as char);
        out.push(ALPHABET[(n & 0x3F) as usize] as char);
        i += 3;
    }
    let rem = input.len() - i;
    if rem == 1 {
        let n = (input[i] as u32) << 16;
        out.push(ALPHABET[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3F) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rem == 2 {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8);
        out.push(ALPHABET[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3F) as usize] as char);
        out.push(ALPHABET[((n >> 6) & 0x3F) as usize] as char);
        out.push('=');
    }
    out
}

fn warn_oversized_kb_articles() {
    let dir = Path::new("knowledgebase").join("articles");
    if !dir.is_dir() {
        return;
    }
    // Re-run when articles change so the warning stays in sync.
    println!("cargo:rerun-if-changed=knowledgebase/articles");
    println!("cargo:rerun-if-changed=knowledgebase/mappings.json");

    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let len = match std::fs::metadata(&path) {
            Ok(m) => m.len(),
            Err(_) => continue,
        };
        if len > KB_ARTICLE_MAX_BYTES {
            println!(
                "cargo:warning=knowledgebase article '{}' is {} bytes (>{}); consider trimming",
                path.display(),
                len,
                KB_ARTICLE_MAX_BYTES
            );
        }
    }
}

/// Generate `${OUT_DIR}/scoutsuite_pin.rs` containing
/// `pub const PLATFORM_PINNED_SHA256: Option<&str>`.
///
/// Reads `vendor/scoutsuite/pinned.json` (CI-staged) and validates that
/// its `triple` field matches the current `cargo` target so a stale
/// pinned.json from a different platform fails the build, not the
/// runtime integrity check.
///
/// JSON shape produced by the release workflow's `stage bundled
/// scoutsuite` step:
///   {"binary": "scoutsuite", "triple": "<target-triple>", "sha256": "<hex>"}
fn generate_scoutsuite_pin() {
    let pinned_path = Path::new("vendor").join("scoutsuite").join("pinned.json");
    println!("cargo:rerun-if-changed={}", pinned_path.display());

    let out_dir = match std::env::var_os("OUT_DIR") {
        Some(d) => PathBuf::from(d),
        None => return, // not a cargo build — nothing to do
    };
    let out_file = out_dir.join("scoutsuite_pin.rs");

    let body = match read_scoutsuite_pin(&pinned_path) {
        Ok(Some(sha)) => {
            format!("pub const PLATFORM_PINNED_SHA256: Option<&str> = Some(\"{sha}\");\n",)
        }
        Ok(None) => {
            // No pinned.json — typical dev build. `availability()` will
            // return `Missing` because `verify_sha256()` short-circuits
            // on a None pin. The dev env-var seam
            // (CLOUDSAW_SCOUTSUITE_SHA256_OVERRIDE) is the escape hatch.
            "pub const PLATFORM_PINNED_SHA256: Option<&str> = None;\n".to_string()
        }
        Err(msg) => {
            // Hard-fail the build. Anything other than "file is absent"
            // is a misconfiguration we want a human to see — silently
            // falling back to `None` would mask a CI step that wrote a
            // malformed pinned.json or a triple that doesn't match the
            // build target.
            panic!("invalid vendor/scoutsuite/pinned.json: {msg}");
        }
    };

    if let Err(e) = std::fs::write(&out_file, body) {
        panic!("failed to write {}: {e}", out_file.display(),);
    }
}

/// Tauri's `bundle.resources` glob validation fails the build if a
/// pattern matches zero files. In dev builds nothing has been staged at
/// `vendor/scoutsuite/<triple>/scoutsuite[.exe]`, so the glob errors
/// before `tauri build` even starts. To keep dev builds working without
/// requiring every contributor to manually stage a binary, this function
/// writes a zero-byte placeholder at the target-triple-specific path so
/// the glob matches.
///
/// The placeholder has SHA-256
/// `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`
/// (the empty-string digest). That cannot match
/// `PLATFORM_PINNED_SHA256` (which is `None` in dev builds anyway), so
/// `verify_sha256()` returns `NotBundled` / `IntegrityFailed` at runtime
/// and the UI reports the binary as missing — exactly the existing dev
/// UX. The CI release workflow overwrites the placeholder with the real
/// PyInstaller-frozen binary before `tauri build` runs, so production
/// installers ship the real thing.
fn ensure_scoutsuite_bundle_target() {
    let target = match std::env::var("TARGET") {
        Ok(t) => t,
        Err(_) => return, // not a cargo build — nothing to ensure
    };
    let triple_dir = Path::new("vendor").join("scoutsuite").join(&target);
    let exe_name = if target.contains("windows") {
        "scoutsuite.exe"
    } else {
        "scoutsuite"
    };
    let target_path = triple_dir.join(exe_name);

    if target_path.exists() {
        // Either CI staged the real binary (release path) or a previous
        // dev iteration already created the placeholder. Leave it alone
        // — overwriting a real binary with a 0-byte file here would be
        // catastrophic.
        return;
    }

    if let Err(e) = std::fs::create_dir_all(&triple_dir) {
        // Non-fatal: dev builds will fail at the tauri-build glob step
        // with the same error we're trying to prevent. Print so the
        // failure mode is at least visible.
        println!(
            "cargo:warning=failed to create scoutsuite placeholder dir {}: {e}",
            triple_dir.display()
        );
        return;
    }
    if let Err(e) = std::fs::write(&target_path, b"") {
        println!(
            "cargo:warning=failed to write scoutsuite placeholder {}: {e}",
            target_path.display()
        );
    }
}

/// Returns `Ok(Some(sha))` when pinned.json exists and validates against
/// the current target, `Ok(None)` when the file is absent, `Err(msg)` on
/// any structural or triple-mismatch problem.
fn read_scoutsuite_pin(path: &Path) -> Result<Option<String>, String> {
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("read failed: {e}")),
    };
    let parsed: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("JSON parse: {e}"))?;
    let triple = parsed
        .get("triple")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing `triple` field".to_string())?;
    let sha = parsed
        .get("sha256")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing `sha256` field".to_string())?;

    // Cargo sets TARGET to the build's target triple. Reject if the
    // pinned.json was produced for a different platform — the runtime
    // integrity check would fail unhelpfully later; failing here points
    // straight at the CI mis-step.
    let target = std::env::var("TARGET").map_err(|e| format!("cargo did not set TARGET: {e}"))?;
    if triple != target {
        return Err(format!(
            "pinned.json triple `{triple}` does not match cargo target `{target}` \
             — the CI `stage bundled scoutsuite` step staged the wrong platform's binary"
        ));
    }

    if sha.len() != 64 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("sha256 `{sha}` is not a 64-char hex digest"));
    }
    Ok(Some(sha.to_ascii_lowercase()))
}
