// Contract 17-QA — CredentialStore Abstraction & CI-Testable Keychain
// Seam.
//
// The contract is a refactor, not a feature addition. These tests
// verify:
//
//   * Trait parity — `KeyringStore` and `InMemoryStore` expose the
//     same observable surface (set → get → delete → list → delete_all),
//     so a test that passes against one would pass against the other.
//     We exercise the trait contract directly against `InMemoryStore`
//     (the production impl is exercised in the `#[ignore]`d smoke).
//   * Feature module wiring — `github::pat`, `ai::key`, and the
//     `wipe::run_panic_wipe` path read through the installed store
//     and never touch `keyring` directly. We confirm by running a
//     real round-trip against the in-memory store: set a "PAT",
//     read it back, delete-all, read returns None.
//   * In-memory state isolation — `install_in_memory_for_tests`
//     returns a fresh, empty store on every call within a serialized
//     test process.
//   * `#[ignore]`d smoke test — confirms the production
//     `KeyringStore` actually reads and writes the OS keychain on a
//     developer machine. Marked ignored so CI never runs it.

use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use cloudsaw_lib::keychain::{
    self, CredentialStore, InMemoryStore, KeyringStore, GITHUB_PAT_ACCOUNT, GITHUB_PAT_SERVICE,
    LLM_KEY_ACCOUNT_ANTHROPIC, LLM_KEY_ACCOUNT_OPENAI, LLM_KEY_SERVICE,
};

fn env_lock() -> &'static Mutex<()> {
    static L: OnceLock<Mutex<()>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(()))
}

// --- Happy Path — trait round-trip parity --------------------------------

#[test]
fn happy_in_memory_set_get_delete_round_trip() {
    let _g = env_lock().lock().unwrap_or_else(|p| p.into_inner());
    let store = InMemoryStore::new();
    // Empty store → get returns None.
    assert!(matches!(store.get("svc", "acct"), Ok(None)));
    // Set then get.
    store.set("svc", "acct", "secret").unwrap();
    assert_eq!(store.get("svc", "acct").unwrap().as_deref(), Some("secret"));
    // Delete returns Ok(true) for present, Ok(false) for absent.
    assert!(matches!(store.delete("svc", "acct"), Ok(true)));
    assert!(matches!(store.delete("svc", "acct"), Ok(false)));
    // Subsequent get is None.
    assert!(matches!(store.get("svc", "acct"), Ok(None)));
}

#[test]
fn happy_list_known_returns_the_module_registry() {
    let _g = env_lock().lock().unwrap_or_else(|p| p.into_inner());
    let store = InMemoryStore::new();
    let listed = store.list_known();
    // Contract 12 + 13 registered entries.
    assert!(listed
        .iter()
        .any(|(s, a)| *s == GITHUB_PAT_SERVICE && *a == GITHUB_PAT_ACCOUNT));
    assert!(listed
        .iter()
        .any(|(s, a)| *s == LLM_KEY_SERVICE && *a == LLM_KEY_ACCOUNT_ANTHROPIC));
    assert!(listed
        .iter()
        .any(|(s, a)| *s == LLM_KEY_SERVICE && *a == LLM_KEY_ACCOUNT_OPENAI));
    // The free `registry_snapshot` returns the same content.
    assert_eq!(keychain::registry_snapshot(), listed);
}

#[test]
fn happy_delete_all_empties_the_store_and_counts_outcomes() {
    let _g = env_lock().lock().unwrap_or_else(|p| p.into_inner());
    let store = InMemoryStore::new();
    // Pre-populate with the two registered LLM entries; leave the
    // GitHub PAT absent so we can assert the not_present counter.
    store
        .set(LLM_KEY_SERVICE, LLM_KEY_ACCOUNT_ANTHROPIC, "k1")
        .unwrap();
    store
        .set(LLM_KEY_SERVICE, LLM_KEY_ACCOUNT_OPENAI, "k2")
        .unwrap();
    let result = store.delete_all();
    assert_eq!(result.removed, 2);
    assert_eq!(result.not_present, 1);
    assert_eq!(result.failed, 0);
    // Store is empty afterwards.
    assert!(store.is_empty());
}

// --- Module-level shims delegate to the installed store -----------------

#[test]
fn happy_free_function_shims_delegate_to_installed_in_memory_store() {
    let _g = env_lock().lock().unwrap_or_else(|p| p.into_inner());
    let handle = keychain::install_in_memory_for_tests();
    // Set via the free function; the in-memory store handle observes
    // the same value.
    keychain::set("svc", "acct", "via-shim").unwrap();
    assert_eq!(
        handle.get("svc", "acct").unwrap().as_deref(),
        Some("via-shim")
    );
    // Get via the free function.
    assert_eq!(
        keychain::get("svc", "acct").unwrap().as_deref(),
        Some("via-shim")
    );
    // Delete via the free function.
    assert!(matches!(keychain::delete("svc", "acct"), Ok(true)));
    assert!(matches!(keychain::get("svc", "acct"), Ok(None)));
}

#[test]
fn happy_install_in_memory_for_tests_returns_a_fresh_view_each_call() {
    let _g = env_lock().lock().unwrap_or_else(|p| p.into_inner());
    let first = keychain::install_in_memory_for_tests();
    first.set("svc", "acct", "x").unwrap();
    assert_eq!(first.len(), 1);
    // Second call clears the existing store.
    let _second = keychain::install_in_memory_for_tests();
    assert!(first.is_empty());
}

// --- Feature wiring (Contract 11/12/13 paths read through the trait) ----

#[test]
fn happy_github_pat_module_round_trips_through_the_installed_store() {
    let _g = env_lock().lock().unwrap_or_else(|p| p.into_inner());
    let _ = keychain::install_in_memory_for_tests();
    let probe = "ghp_aaaaaaaaaaaaaaaaaaaaa";
    cloudsaw_lib::github::pat::set(zeroize::Zeroizing::new(probe.to_string())).unwrap();
    let got = cloudsaw_lib::github::pat::get().unwrap();
    assert_eq!(got.as_deref().map(|z| z.as_str()), Some(probe));
    cloudsaw_lib::github::pat::clear().unwrap();
    let got = cloudsaw_lib::github::pat::get().unwrap();
    assert!(got.is_none());
}

#[test]
fn happy_ai_key_module_round_trips_through_the_installed_store() {
    let _g = env_lock().lock().unwrap_or_else(|p| p.into_inner());
    let _ = keychain::install_in_memory_for_tests();
    let probe = "sk-ant-aaaaaaaaaaaaaaaa";
    cloudsaw_lib::ai::key::set(
        cloudsaw_lib::ai::Provider::Anthropic,
        zeroize::Zeroizing::new(probe.to_string()),
    )
    .unwrap();
    assert!(cloudsaw_lib::ai::key::has(cloudsaw_lib::ai::Provider::Anthropic).unwrap());
    cloudsaw_lib::ai::key::clear(cloudsaw_lib::ai::Provider::Anthropic).unwrap();
    assert!(!cloudsaw_lib::ai::key::has(cloudsaw_lib::ai::Provider::Anthropic).unwrap());
}

#[test]
fn happy_wipe_all_empties_every_registered_entry_via_the_installed_store() {
    let _g = env_lock().lock().unwrap_or_else(|p| p.into_inner());
    let handle = keychain::install_in_memory_for_tests();
    // Populate the GitHub PAT + both LLM entries.
    keychain::set(GITHUB_PAT_SERVICE, GITHUB_PAT_ACCOUNT, "pat").unwrap();
    keychain::set(LLM_KEY_SERVICE, LLM_KEY_ACCOUNT_ANTHROPIC, "ant").unwrap();
    keychain::set(LLM_KEY_SERVICE, LLM_KEY_ACCOUNT_OPENAI, "oai").unwrap();
    assert_eq!(handle.len(), 3);
    let result = keychain::wipe_all();
    assert_eq!(result.removed, 3);
    assert_eq!(result.not_present, 0);
    assert_eq!(result.failed, 0);
    assert!(handle.is_empty());
}

// --- Security Check -----------------------------------------------------

#[test]
fn security_no_feature_module_references_keyring_directly() {
    // Walk the src tree (excluding the keychain dir, vendor, etc.)
    // and assert no file other than `keychain/store_keyring.rs`
    // references `keyring::*`. This enforces Contract 17 §Constraints
    // even if a future PR refactors a feature module without
    // realizing the rule.
    use std::path::Path;
    fn walk(dir: &Path, hits: &mut Vec<String>) {
        let Ok(reader) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in reader.flatten() {
            let path = entry.path();
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            if matches!(name.as_str(), "target" | "node_modules" | ".git" | "vendor") {
                continue;
            }
            if path.is_dir() {
                walk(&path, hits);
                continue;
            }
            // Skip the production impl — it's the one allowed
            // reference site.
            if path.ends_with("store_keyring.rs") {
                continue;
            }
            // Only audit .rs files. Comment-level mentions of
            // "keyring" in non-Rust files (Markdown, JSON) don't
            // count as a direct usage.
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            // Skip THIS test file (it intentionally mentions the
            // forbidden token as part of the audit).
            if path.ends_with("qa17_test.rs") {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            // The audit matches references to the actual `keyring`
            // crate's public API (`Entry`, `Error`, the use-path
            // form). It does NOT match the substring `keyring::` in
            // identifiers like `store_keyring::KeyringStore`, which
            // is a local module name.
            let direct_use = content.contains("keyring::Entry")
                || content.contains("keyring::Error")
                || content.contains("use keyring;")
                || content.contains("extern crate keyring");
            if direct_use {
                hits.push(path.display().to_string());
            }
        }
    }

    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("src-tauri")
        .join("src");
    let mut hits = Vec::new();
    walk(&repo_root, &mut hits);
    assert!(
        hits.is_empty(),
        "Contract 17 §Constraints: only `keychain/store_keyring.rs` may reference `keyring::*` — found:\n  {}",
        hits.join("\n  ")
    );
}

#[test]
fn security_in_memory_store_does_not_persist_to_disk() {
    // Sanity: the InMemoryStore does NOT write anywhere. Drop the
    // handle, the value is gone. We assert this by setting a probe,
    // dropping the store, creating a fresh one, and confirming the
    // probe isn't visible.
    let _g = env_lock().lock().unwrap_or_else(|p| p.into_inner());
    {
        let store = InMemoryStore::new();
        store.set("svc", "acct", "ephemeral").unwrap();
    } // store dropped here
    let store2 = InMemoryStore::new();
    assert!(matches!(store2.get("svc", "acct"), Ok(None)));
}

// --- #[ignore]d real-keychain smoke test --------------------------------
//
// Contract 17 §Expected Output #6: exactly one real-keychain smoke
// test, marked `#[ignore]` so CI never runs it. A developer running
// it on their own machine confirms the production `KeyringStore`
// genuinely uses the OS keychain.
//
// Run manually:  cargo test --test qa17_test -- --ignored
//
// The probe uses a unique service name (random nanosecond suffix)
// so two concurrent test invocations on the same machine don't
// collide, and the test ALWAYS deletes its entry on success or
// failure (RAII guard).

#[test]
#[ignore = "real-keychain smoke: only run on demand on a developer machine — never in CI"]
fn smoke_real_keyring_round_trip_then_cleanup() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let service = format!("cloudsaw.test.smoke.{nanos}");
    let account = "qa17";
    let secret = "real-keychain-probe-value";

    // RAII guard: on drop, attempt to delete the entry even if the
    // assertions above panicked, so we don't leak rows into the
    // developer's OS keychain.
    struct Cleanup<'a> {
        store: &'a KeyringStore,
        service: String,
        account: &'a str,
    }
    impl Drop for Cleanup<'_> {
        fn drop(&mut self) {
            let _ = self.store.delete(&self.service, self.account);
        }
    }

    let store = KeyringStore::new();
    let _cleanup = Cleanup {
        store: &store,
        service: service.clone(),
        account,
    };

    // No prior entry.
    let got = store.get(&service, account).unwrap();
    assert!(got.is_none(), "smoke probe service must not pre-exist");

    // Set, then get back.
    store.set(&service, account, secret).unwrap();
    let got = store.get(&service, account).unwrap();
    assert_eq!(got.as_deref(), Some(secret));

    // Delete returns true, and subsequent get is None.
    let removed = store.delete(&service, account).unwrap();
    assert!(removed);
    let got = store.get(&service, account).unwrap();
    assert!(got.is_none());
}
