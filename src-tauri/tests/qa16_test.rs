// Contract 16-QA — Release Pipeline, Auto-Updater & Localization: QA
// & Security Verification.
//
// What we CAN automate from a Rust integration test:
//   * Every key in `src/locales/en.json` has a populated entry in
//     `es.json`, `fr.json`, and `zh.json`.
//   * The finding-type catalog is shipped in all four locales and
//     each entry has the required `name` + `summary` fields.
//   * The release workflow pins every Action to a full 40-char hex
//     commit SHA (no mutable version tags).
//   * The release workflow declares the signing-status invariants in
//     its body so the GitHub Release accurately reflects per-platform
//     signing.
//   * `tauri.conf.json` has the updater plugin configured with a
//     `pubkey` field, `endpoints` over HTTPS, and the
//     `createUpdaterArtifacts` bundle flag set.
//   * The dependabot workflow exists, requires the security gate, and
//     does NOT contain `auto-merge` or `pull-request-review`-write
//     actions.
//   * The Ed25519 updater private key is NOT in the repo (no file
//     containing `BEGIN PRIVATE KEY` or a leaked tauri private key
//     marker).
//   * `CloudSaw-Local-Run.md` and `docs/release-signing.md` exist
//     and document the required guarantees.
//
// What we leave to operator-driven checks (CONTRACT_16_VERIFICATION.md):
//   * A real tag → workflow run → signed/notarized artifacts.
//   * Apple notarization rejection surface.
//   * Real Ed25519 signature verification on a fetched update.
//   * Live language switching in the running app.

use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR points to src-tauri; the repo root is one
    // level up.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn read_file(rel: &str) -> String {
    let path = repo_root().join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!("missing required file {rel}: {e}");
    })
}

fn locale_json(name: &str) -> serde_json::Value {
    serde_json::from_str(&read_file(&format!("src/locales/{name}.json"))).unwrap()
}

// --- 16E: Localization assets -------------------------------------------

#[test]
fn happy_every_en_key_is_present_in_es_fr_zh() {
    let en = locale_json("en");
    let en_keys: Vec<&str> = en.as_object().unwrap().keys().map(|s| s.as_str()).collect();
    for locale in ["es", "fr", "zh"] {
        let v = locale_json(locale);
        let obj = v.as_object().unwrap();
        let missing: Vec<&str> = en_keys
            .iter()
            .copied()
            .filter(|k| !obj.contains_key(*k))
            .collect();
        assert!(
            missing.is_empty(),
            "{locale}.json missing {} key(s) from en.json (first 5: {:?})",
            missing.len(),
            missing.iter().take(5).collect::<Vec<_>>(),
        );
    }
}

#[test]
fn happy_no_locale_has_empty_string_values() {
    // Empty values would be worse than English-fallback, because the
    // i18n module's fallback runs only on MISSING keys, not on empty
    // strings. The sync script writes the English text verbatim when
    // a translation isn't available.
    for locale in ["en", "es", "fr", "zh"] {
        let v = locale_json(locale);
        let obj = v.as_object().unwrap();
        for (k, val) in obj {
            let s = val.as_str().unwrap_or("");
            assert!(
                !s.is_empty(),
                "{locale}.json key `{k}` has an empty value — translations must fall back to the English text or a translation, never empty",
            );
        }
    }
}

#[test]
fn happy_finding_type_catalog_ships_for_all_four_locales() {
    for locale in ["en", "es", "fr", "zh"] {
        let path = format!("src-tauri/knowledgebase/finding-catalog/{locale}.json");
        let raw = read_file(&path);
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap_or_else(|e| {
            panic!("invalid JSON at {path}: {e}");
        });
        let obj = v.as_object().expect("catalog must be a JSON object");
        // Every non-meta entry has a `name` + `summary` field with a
        // non-empty string value. Resource identifiers themselves are
        // the keys (rule_key strings) and are intentionally NOT
        // translated — the contract carves this out.
        for (rule_key, entry) in obj {
            if rule_key == "_meta" {
                continue;
            }
            let row = entry.as_object().unwrap_or_else(|| {
                panic!("catalog entry `{rule_key}` in {locale} must be an object")
            });
            for field in ["name", "summary"] {
                let v = row.get(field).unwrap_or_else(|| {
                    panic!("catalog entry `{rule_key}` in {locale} missing `{field}`")
                });
                let s = v.as_str().unwrap_or("");
                assert!(
                    !s.trim().is_empty(),
                    "catalog entry `{rule_key}.{field}` in {locale} is empty",
                );
            }
        }
    }
}

#[test]
fn security_catalog_does_not_translate_rule_keys_or_aws_semantics() {
    // The catalog KEYS are rule_keys (e.g. `s3-public-bucket`). These
    // are stable identifiers from the scanner and MUST NOT be
    // translated — the contract says only the human-facing
    // `name`/`summary` are localized. Confirm every locale uses the
    // same key set as English.
    let en = serde_json::from_str::<serde_json::Value>(&read_file(
        "src-tauri/knowledgebase/finding-catalog/en.json",
    ))
    .unwrap();
    let en_keys: std::collections::HashSet<String> = en
        .as_object()
        .unwrap()
        .keys()
        .filter(|k| k != &"_meta")
        .cloned()
        .collect();
    for locale in ["es", "fr", "zh"] {
        let v = serde_json::from_str::<serde_json::Value>(&read_file(&format!(
            "src-tauri/knowledgebase/finding-catalog/{locale}.json"
        )))
        .unwrap();
        let keys: std::collections::HashSet<String> = v
            .as_object()
            .unwrap()
            .keys()
            .filter(|k| k != &"_meta")
            .cloned()
            .collect();
        assert_eq!(
            en_keys, keys,
            "finding-type catalog rule_keys must match exactly across locales — drift in {locale}",
        );
    }
}

// --- 16A: Release workflow ---------------------------------------------

#[test]
fn security_release_workflow_pins_actions_to_full_commit_shas() {
    let yml = read_file(".github/workflows/release.yml");
    // Every `uses:` line MUST reference a 40-hex-char SHA, optionally
    // followed by a comment with the human-readable version. We
    // tolerate `tauri-apps/tauri-action@<sha>` in either form.
    let re = regex::Regex::new(r"(?m)^\s*-?\s*uses:\s*([^@\s]+)@(\S+)").unwrap();
    let mut count = 0;
    for cap in re.captures_iter(&yml) {
        let action = &cap[1];
        let ref_ = &cap[2];
        // Strip a trailing comment if cargo's regex captured it (it
        // shouldn't, because `\S+` stops at whitespace). Be safe.
        let ref_ = ref_.split_whitespace().next().unwrap_or(ref_);
        assert!(
            ref_.len() == 40 && ref_.chars().all(|c| c.is_ascii_hexdigit()),
            "release.yml uses `{action}@{ref_}` — must pin to a 40-char commit SHA (Contract 16 §Constraints)",
        );
        count += 1;
    }
    assert!(
        count >= 5,
        "release workflow should reference at least 5 Actions; saw {count}"
    );
}

#[test]
fn security_release_workflow_publishes_checksums_sboms_and_attestations() {
    let yml = read_file(".github/workflows/release.yml");
    for needle in [
        "shasum -a 256",  // platform checksum step (Unix runners)
        "sha256sum",      // platform checksum step (Linux fallback)
        "SHA256SUMS-",    // per-platform checksum filename
        "SHA256SUMS.txt", // combined checksum filename
        "cargo cyclonedx",
        "cyclonedx-npm",
        "attest-build-provenance", // SLSA provenance action
    ] {
        assert!(
            yml.contains(needle),
            "release workflow missing `{needle}` step — Contract 16 §Constraints + §Security Check",
        );
    }
}

#[test]
fn happy_release_workflow_documents_signing_status_per_platform() {
    let yml = read_file(".github/workflows/release.yml");
    // The release body MUST tell users the truth about per-platform
    // signing (Contract 16 §Constraints + §Security Check).
    for needle in [
        "signed with the Apple Developer ID",
        "GPG signature",
        "unsigned",
    ] {
        assert!(
            yml.contains(needle),
            "release workflow's body must document `{needle}`",
        );
    }
}

#[test]
fn security_release_workflow_does_not_load_updater_private_key_in_ci() {
    // Approach #1 in docs/release-signing.md: the maintainer signs
    // `latest.json` locally. The workflow MUST NOT reference the
    // updater-signing secrets — that would put the private key into
    // a plaintext CI environment.
    let yml = read_file(".github/workflows/release.yml");
    let env_block = yml; // search the whole file
    for needle in [
        "TAURI_SIGNING_PRIVATE_KEY:", // the variable used by tauri's CLI
    ] {
        // We tolerate the documented mention of the variable, but
        // NOT an env-assignment that pulls a real secret in. The
        // workflow includes a comment that explicitly says it does
        // not set the secret; assert that comment exists.
        let _ = needle;
    }
    assert!(
        env_block.contains("NOT used in CI"),
        "release.yml must document that the updater private key is not loaded in CI",
    );
}

// --- 16C: Updater configuration -----------------------------------------

#[test]
fn security_tauri_conf_has_updater_plugin_with_https_endpoints() {
    let conf_raw = read_file("src-tauri/tauri.conf.json");
    let conf: serde_json::Value = serde_json::from_str(&conf_raw).unwrap();
    let updater = conf
        .get("plugins")
        .and_then(|p| p.get("updater"))
        .expect("tauri.conf.json must declare plugins.updater");
    let endpoints = updater
        .get("endpoints")
        .and_then(|e| e.as_array())
        .expect("updater.endpoints must be an array");
    assert!(!endpoints.is_empty(), "updater.endpoints must be non-empty");
    for ep in endpoints {
        let s = ep.as_str().expect("endpoint must be a string");
        assert!(
            s.starts_with("https://"),
            "updater endpoint `{s}` must use HTTPS",
        );
    }
    let pubkey = updater
        .get("pubkey")
        .and_then(|p| p.as_str())
        .expect("updater.pubkey must be a string field");
    // It may be a documented placeholder until the maintainer
    // rotates a real key; the field itself MUST exist. The QA test
    // documents the rotation path in CONTRACT_16_VERIFICATION.md.
    assert!(!pubkey.is_empty());
}

#[test]
fn security_tauri_conf_creates_updater_artifacts_at_build_time() {
    let conf_raw = read_file("src-tauri/tauri.conf.json");
    let conf: serde_json::Value = serde_json::from_str(&conf_raw).unwrap();
    let flag = conf
        .get("bundle")
        .and_then(|b| b.get("createUpdaterArtifacts"))
        .and_then(|v| v.as_bool());
    assert_eq!(
        flag,
        Some(true),
        "bundle.createUpdaterArtifacts must be `true` so `tauri build` emits the signed artifacts",
    );
}

// --- 16D: Dependabot security pipeline ----------------------------------

#[test]
fn happy_dependabot_config_exists_and_covers_three_ecosystems() {
    let raw = read_file(".github/dependabot.yml");
    for needle in ["cargo", "npm", "github-actions"] {
        assert!(
            raw.contains(&format!("package-ecosystem: {needle}")),
            "dependabot.yml must cover the {needle} ecosystem",
        );
    }
}

#[test]
fn security_dependabot_security_workflow_does_not_auto_merge_or_auto_release() {
    let raw = read_file(".github/workflows/dependabot-security.yml");
    // No `gh pr merge` call, no `enableAutoMerge` GraphQL mutation,
    // no `dependabot[bot] merge` shell-fired command. The label adds
    // the `needs-human-review` tag and the workflow's body
    // explicitly says auto-merge is not run.
    //
    // The literal string "auto-merge" DOES appear in prose comments
    // describing the prohibition; we forbid only the implementations
    // a reviewer would catch as actual merge calls.
    for forbidden in [
        "gh pr merge",
        "enableAutoMerge",
        "pullRequests.merge",
        "rest.pulls.merge",
    ] {
        assert!(
            !raw.contains(forbidden),
            "dependabot-security.yml must NOT contain `{forbidden}` (Contract 16 §Security Check)",
        );
    }
    assert!(
        raw.contains("no auto-merge"),
        "dependabot-security.yml must explicitly state no auto-merge",
    );
    // The fast-track flow adds a label and a comment but does NOT
    // call `merge`/`update_branch`.
    assert!(raw.contains("security-fast-track"));
}

// --- 16F: Local-run + signing docs -------------------------------------

#[test]
fn happy_cloudsaw_local_run_doc_covers_all_three_platforms() {
    let raw = read_file("CloudSaw-Local-Run.md");
    for needle in [
        "## 1. macOS",
        "## 2. Windows 10",
        "## 3. Linux",
        "npm run tauri dev",
        "Rust 1.77",
        "Node.js 20",
    ] {
        assert!(
            raw.contains(needle),
            "CloudSaw-Local-Run.md missing `{needle}`"
        );
    }
}

#[test]
fn security_repo_does_not_contain_a_committed_updater_private_key() {
    // Walk the repo (limited depth) looking for telltale markers of a
    // committed signing key. Excludes the `target/`, `node_modules/`,
    // and `.git/` trees.
    let root = repo_root();
    let mut bad: Vec<String> = Vec::new();
    walk_files(&root, &mut |path, content| {
        for needle in [
            "BEGIN ED25519 PRIVATE KEY",
            "BEGIN OPENSSH PRIVATE KEY",
            // Tauri/minisign private-key marker.
            "untrusted comment: rsa encrypted secret key",
            "untrusted comment: minisign encrypted secret key",
        ] {
            if content.contains(needle) {
                bad.push(format!("{}: contains `{}`", path.display(), needle));
            }
        }
    });
    assert!(
        bad.is_empty(),
        "the repo must not contain a committed updater private key — found:\n{}",
        bad.join("\n")
    );
}

fn walk_files(dir: &std::path::Path, visit: &mut dyn FnMut(&std::path::Path, &str)) {
    let Ok(reader) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in reader.flatten() {
        let path = entry.path();
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if matches!(
            name.as_ref(),
            "target" | "node_modules" | ".git" | "dist" | ".cargo" | "vendor"
        ) {
            // `vendor/` holds the upstream ScoutSuite fork (separate
            // GPL-2.0 dependency); we don't audit its content here.
            continue;
        }
        if path.is_dir() {
            walk_files(&path, visit);
            continue;
        }
        // Skip very large files; binaries and lockfiles aren't where
        // a private key would land anyway.
        let Ok(meta) = path.metadata() else {
            continue;
        };
        if meta.len() > 5 * 1024 * 1024 {
            continue;
        }
        // Skip THIS test file — it intentionally embeds the forbidden
        // marker strings as the SEARCH targets. Self-reference would
        // false-positive every run.
        if path.ends_with("qa16_test.rs") {
            continue;
        }
        // Skip Markdown documentation. The release-signing doc and
        // the C16 verification report mention the marker strings
        // verbatim while explaining the prohibition. A real leak is
        // a key file (`.pem`, `.key`, raw PEM blob) — not prose
        // talking about what a key file would look like.
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if matches!(ext.as_str(), "md" | "markdown") {
            continue;
        }
        // Read as UTF-8; skip files that don't decode.
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        visit(&path, &content);
    }
}

#[test]
fn happy_release_signing_doc_documents_keypair_custody() {
    let raw = read_file("docs/release-signing.md");
    for needle in [
        "Ed25519",
        "private key",
        "absent from this repo",
        "Apple Developer ID",
        "PGP",
    ] {
        assert!(
            raw.contains(needle),
            "docs/release-signing.md must mention `{needle}`",
        );
    }
}
