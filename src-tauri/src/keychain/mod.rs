// Credential store (Contract 17 — remediation of Contracts 11/12/13).
//
// CLAUDE.md §4.3: "API keys, GitHub PATs, and similar secrets live ONLY
// in the OS keychain. They are fetched on demand and held in memory for
// the minimum time needed."
//
// CLAUDE.md §4.8 (added by Contract 17): "Any code that touches an
// OS-provided facility — the OS keychain / credential store, biometric
// APIs, native OS dialogs, or similar — MUST sit behind an injectable
// abstraction (a trait), not call the OS facility directly."
//
// This module is that abstraction for the OS keychain. The trait
// `CredentialStore` is the ONLY credential interface feature modules
// see. Two implementations satisfy it:
//
//   * `KeyringStore` — production. Wraps the `keyring` crate's
//     per-entry API. The macOS Keychain, Windows Credential Manager,
//     and Linux Secret Service backends are unchanged from before
//     Contract 17.
//   * `InMemoryStore` — test only. Backed by an in-process `RwLock`
//     map. No `keyring` dependency, no D-Bus, no desktop session
//     required. A CI runner can exercise the full feature logic
//     against it deterministically.
//
// Feature code (`github::pat`, `ai::key`, `wipe::*`) calls the free
// functions `get`/`set`/`delete`/`wipe_all`/`registry_snapshot` on
// this module; the free functions delegate to the installed store
// via the `store()` accessor. `install_store(...)` is called exactly
// once per process — by `lib::run` for the production app and by
// each test's sandbox setup for the in-memory variant.
//
// Why a registry: when the Contract 11 panic action runs, we cannot
// ask the OS keychain "list every entry that begins with 'cloudsaw'"
// portably — neither the macOS Keychain nor the Linux Secret Service
// exposes a "wildcard delete" API, and Windows Credential Manager's
// enumeration is a separate, generic-only surface. So every contract
// that stores a secret declares its (service, account) pair in
// `REGISTRY`. The panic-wipe path iterates the registry and tries
// each entry — entries that were never written silently succeed.

mod store_keyring;
mod store_memory;

use std::sync::Arc;
use std::sync::OnceLock;

use serde::Serialize;
use thiserror::Error;

pub use store_keyring::KeyringStore;
pub use store_memory::InMemoryStore;

use crate::errors::AppError;

/// Canonical service name for the GitHub fine-grained PAT (Contract 12).
pub const GITHUB_PAT_SERVICE: &str = "cloudsaw.github_pat";
pub const GITHUB_PAT_ACCOUNT: &str = "default";

/// Canonical service name for AI provider API keys (Contract 13).
pub const LLM_KEY_SERVICE: &str = "cloudsaw.llm_api_key";
pub const LLM_KEY_ACCOUNT_ANTHROPIC: &str = "anthropic";
pub const LLM_KEY_ACCOUNT_OPENAI: &str = "openai";
// PR #77 — Gemini lives under the same llm_api_key service, with
// `account` = `gemini` so the legacy single-provider model can still
// route by type while the multi-provider model (PR #74) routes by
// the random per-row `provider_id`. The panic wipe enumerates both
// the static REGISTRY below and the dynamic ai_providers table rows
// so every keychain entry is reachable from a single wipe.
pub const LLM_KEY_ACCOUNT_GEMINI: &str = "gemini";

/// All service/account pairs CloudSaw is permitted to write to the OS
/// keychain. The panic wipe enumerates this list and removes every
/// entry. Adding a new contract that stores a secret means appending
/// a line here AND wiring the read/write through this module.
const REGISTRY: &[(&str, &str)] = &[
    (GITHUB_PAT_SERVICE, GITHUB_PAT_ACCOUNT),
    (LLM_KEY_SERVICE, LLM_KEY_ACCOUNT_ANTHROPIC),
    (LLM_KEY_SERVICE, LLM_KEY_ACCOUNT_OPENAI),
    (LLM_KEY_SERVICE, LLM_KEY_ACCOUNT_GEMINI),
];

/// Typed error returned by every `CredentialStore` method. The inner
/// string is the underlying backend's message, already free of
/// credential material (the backends only emit short status text).
#[derive(Debug, Error)]
pub enum KeychainError {
    #[error("keychain: {0}")]
    Backend(String),
}

impl From<KeychainError> for AppError {
    fn from(e: KeychainError) -> Self {
        // Surface as a generic IO-shaped error — the IPC layer rarely
        // shows this directly (the panic flow returns a structured
        // summary), and we never leak provider-specific text.
        AppError::Io(e.to_string())
    }
}

/// Outcome of wiping every CloudSaw-owned keychain entry. `not_present`
/// counts entries the registry knew about but which weren't actually
/// stored (the common case for a fresh install).
#[derive(Debug, Default, Clone, Serialize)]
pub struct KeychainWipeResult {
    pub removed: usize,
    pub not_present: usize,
    pub failed: usize,
}

/// The credential operations CloudSaw's feature code needs. Two
/// implementations satisfy this trait (`KeyringStore`, `InMemoryStore`);
/// feature code depends only on the trait, never on either impl
/// directly.
///
/// All methods return `Result` with a typed `KeychainError`; none
/// panic.
pub trait CredentialStore: Send + Sync {
    /// Look up a secret. Returns `Ok(None)` when the entry doesn't
    /// exist — both implementations distinguish "missing" from
    /// "backend error."
    fn get(&self, service: &str, account: &str) -> Result<Option<String>, KeychainError>;

    /// Store a secret, overwriting any existing value for the same
    /// `(service, account)` pair.
    fn set(&self, service: &str, account: &str, secret: &str) -> Result<(), KeychainError>;

    /// Remove a single entry. Returns `Ok(true)` if a row existed,
    /// `Ok(false)` if not. A missing entry is success — Contract 11
    /// §Edge Cases: "Keychain entry already absent at panic time →
    /// the wipe treats this as success, not an error."
    fn delete(&self, service: &str, account: &str) -> Result<bool, KeychainError>;

    /// Enumerate every `(service, account)` pair the store knows
    /// about. The production impl returns the same registry constant
    /// the test impl seeds itself from; both implementations therefore
    /// have the same registry view.
    fn list_known(&self) -> Vec<(&'static str, &'static str)> {
        REGISTRY.to_vec()
    }

    /// Iterate the registry and remove every entry, counting outcomes.
    /// Per-entry failures don't abort the sweep — the Contract 11
    /// panic wipe needs to be as close to "always succeeds" as
    /// possible. The default implementation is the same for both
    /// stores (calls `delete()` for each registered pair); impls can
    /// override if they want a tighter loop.
    fn delete_all(&self) -> KeychainWipeResult {
        let mut out = KeychainWipeResult::default();
        for (service, account) in self.list_known() {
            match self.delete(service, account) {
                Ok(true) => out.removed += 1,
                Ok(false) => out.not_present += 1,
                Err(_) => out.failed += 1,
            }
        }
        out
    }
}

// --- Process-wide accessor --------------------------------------------

/// The currently-installed store. Set ONCE per process — at
/// `lib::run` time for the production app, in each integration test's
/// sandbox setup for tests. `OnceLock` makes the install atomic and
/// the `store()` accessor read-only after install.
static STORE: OnceLock<Arc<dyn CredentialStore>> = OnceLock::new();

/// Install the process-wide credential store. Returns `Err(_)` if a
/// store is already installed (re-install is not allowed; tests
/// should run in their own process / serialized via a mutex). The
/// `running` callers honor this by only calling `install_store` once.
pub fn install_store(store: Arc<dyn CredentialStore>) -> Result<(), AlreadyInstalled> {
    STORE.set(store).map_err(|_| AlreadyInstalled)
}

/// Reset the process-wide store so the next `install_store` succeeds.
/// Test-only: production code never calls this. Used by the integration
/// test sandboxes to swap a fresh `InMemoryStore` between tests.
///
/// Implementation note: `OnceLock` doesn't expose a public reset, so
/// this is a no-op stub when not in test builds. Tests use a
/// per-process Mutex around the install/run/cleanup cycle instead of
/// resetting the static — see `keychain::test_support::with_test_store`.
#[doc(hidden)]
pub fn _reset_for_tests_unreachable() {
    // Intentionally empty. The integration tests serialize the whole
    // sandbox through a Mutex<()> and `install_store` is called once
    // per test process. A test re-running inside the same process
    // (e.g. cargo's `--test-threads=1` rerun semantics) would normally
    // observe a re-install error — the test_support helper handles
    // that by detecting the already-installed state.
}

#[derive(Debug)]
pub struct AlreadyInstalled;

impl std::fmt::Display for AlreadyInstalled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "credential store already installed")
    }
}
impl std::error::Error for AlreadyInstalled {}

/// Read the installed store. Falls back to a fresh `InMemoryStore`
/// when nothing is installed yet — that lets the unit tests inside
/// this crate (which don't go through the sandbox setup) work
/// without a desktop session.
fn store() -> &'static dyn CredentialStore {
    if let Some(s) = STORE.get() {
        return s.as_ref();
    }
    static FALLBACK: OnceLock<Arc<dyn CredentialStore>> = OnceLock::new();
    FALLBACK
        .get_or_init(|| Arc::new(InMemoryStore::new()))
        .as_ref()
}

// --- Free-function shims used by feature code -------------------------
//
// `github::pat`, `ai::key`, and `wipe::run_panic_wipe` call these.
// They delegate to the installed `CredentialStore` — no direct
// `keyring` usage anywhere in feature code.

pub fn get(service: &str, account: &str) -> Result<Option<String>, KeychainError> {
    store().get(service, account)
}

pub fn set(service: &str, account: &str, secret: &str) -> Result<(), KeychainError> {
    store().set(service, account, secret)
}

pub fn delete(service: &str, account: &str) -> Result<bool, KeychainError> {
    store().delete(service, account)
}

pub fn wipe_all() -> KeychainWipeResult {
    store().delete_all()
}

pub fn registry_snapshot() -> Vec<(&'static str, &'static str)> {
    REGISTRY.to_vec()
}

// --- Test helpers -----------------------------------------------------

/// Process-wide handle to the InMemoryStore the tests installed.
/// Production code never reads this. Used by `install_in_memory_for_tests`
/// to clear the existing store between integration tests inside the
/// same `cargo test` process.
static TEST_STORE: OnceLock<Arc<InMemoryStore>> = OnceLock::new();

/// Test-only support: install a fresh `InMemoryStore` for the rest of
/// the test process, or clear the existing one so each test starts
/// from an empty map.
///
/// Integration tests call this from their `Sandbox::new` and serialize
/// through a module-level `Mutex<()>` so two tests never share state.
pub fn install_in_memory_for_tests() -> Arc<InMemoryStore> {
    if let Some(existing) = TEST_STORE.get() {
        // Already installed in this process — clear and reuse.
        existing.clear();
        return existing.clone();
    }
    let fresh = Arc::new(InMemoryStore::new());
    // Best-effort: ignore the result of the public-store install if
    // some other code already raced us (the test_support helper holds
    // a Mutex that prevents this in practice).
    let _ = STORE.set(fresh.clone());
    let _ = TEST_STORE.set(fresh.clone());
    fresh
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_includes_every_registered_pair() {
        let snap = registry_snapshot();
        assert_eq!(snap.len(), REGISTRY.len());
        // Contract 12 registers the GitHub PAT — the registry must include it
        // so the panic wipe sweeps it on the next call.
        assert!(snap
            .iter()
            .any(|(s, a)| *s == GITHUB_PAT_SERVICE && *a == GITHUB_PAT_ACCOUNT));
        // Contract 13 registers the LLM provider keys (one per provider).
        assert!(snap
            .iter()
            .any(|(s, a)| *s == LLM_KEY_SERVICE && *a == LLM_KEY_ACCOUNT_ANTHROPIC));
        assert!(snap
            .iter()
            .any(|(s, a)| *s == LLM_KEY_SERVICE && *a == LLM_KEY_ACCOUNT_OPENAI));
    }

    #[test]
    fn list_known_default_matches_registry_const() {
        // The default impl returns the same `REGISTRY` both stores
        // see. Tests that assert "the panic wipe touches the GitHub
        // PAT + both LLM keys" pass identically against either store.
        let s = InMemoryStore::new();
        assert_eq!(s.list_known(), registry_snapshot());
    }
}
