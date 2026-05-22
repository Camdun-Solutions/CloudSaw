// OS-native secure-storage abstraction.
//
// CLAUDE.md §4.3: "API keys, GitHub PATs, and similar secrets live ONLY
// in the OS keychain. They are fetched on demand and held in memory for
// the minimum time needed." This module owns the small surface every
// other module uses to put/get/delete a CloudSaw-owned secret AND the
// registry the panic wipe enumerates so it can remove every CloudSaw
// entry.
//
// Why a registry: when the panic action runs, we cannot simply ask the
// OS keychain "list every entry that begins with 'cloudsaw'" — neither
// the macOS Keychain nor the Linux Secret Service exposes a portable
// "wildcard delete" API, and Windows Credential Manager's enumeration
// is a separate, generic-only surface. Instead, every contract that
// stores a secret declares its (service, account) pair here. The
// panic-wipe path iterates the registry and tries each — entries that
// were never written silently succeed.
//
// The current registry is empty. Contracts 12 (GitHub Integration) and
// 13 (AI Suggestion Layer) will append their service names.

use serde::Serialize;
use thiserror::Error;

use crate::errors::AppError;

/// Canonical service name for the GitHub fine-grained PAT (Contract 12).
/// Exposed so the `github` module reads/writes the same entry the panic
/// wipe enumerates.
pub const GITHUB_PAT_SERVICE: &str = "cloudsaw.github_pat";
pub const GITHUB_PAT_ACCOUNT: &str = "default";

/// All service/account pairs CloudSaw is permitted to write to the OS
/// keychain. The panic wipe enumerates this list and removes every entry.
///
/// Adding a new contract that stores a secret means appending a line
/// here AND wiring the read/write through `get`/`set`/`delete` below.
const REGISTRY: &[(&str, &str)] = &[
    // (service, account)
    (GITHUB_PAT_SERVICE, GITHUB_PAT_ACCOUNT),
    //
    // Future:
    //   ("cloudsaw.ai_api_key", "anthropic"),
    //   ("cloudsaw.ai_api_key", "openai"),
];

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

/// Look up a secret. Returns `Ok(None)` when the entry doesn't exist.
/// Used by the contracts that store secrets (GitHub PAT, AI key).
#[allow(dead_code)]
pub fn get(service: &str, account: &str) -> Result<Option<String>, KeychainError> {
    match keyring::Entry::new(service, account)
        .map_err(|e| KeychainError::Backend(e.to_string()))?
        .get_password()
    {
        Ok(s) => Ok(Some(s)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(KeychainError::Backend(e.to_string())),
    }
}

/// Store a secret. Overwrites any existing value for the same
/// (service, account) pair.
#[allow(dead_code)]
pub fn set(service: &str, account: &str, secret: &str) -> Result<(), KeychainError> {
    keyring::Entry::new(service, account)
        .map_err(|e| KeychainError::Backend(e.to_string()))?
        .set_password(secret)
        .map_err(|e| KeychainError::Backend(e.to_string()))
}

/// Remove a single entry. Treats `NoEntry` as success — Contract 11
/// §Edge Cases: "Keychain entry already absent at panic time → the wipe
/// treats this as success, not an error."
#[allow(dead_code)]
pub fn delete(service: &str, account: &str) -> Result<bool, KeychainError> {
    match keyring::Entry::new(service, account)
        .map_err(|e| KeychainError::Backend(e.to_string()))?
        .delete_credential()
    {
        Ok(()) => Ok(true),
        Err(keyring::Error::NoEntry) => Ok(false),
        Err(e) => Err(KeychainError::Backend(e.to_string())),
    }
}

/// Iterate every entry in the registry and try to remove it. Per-entry
/// failures don't abort the sweep — the panic wipe needs to be as close
/// to "always succeeds" as possible. Returns the aggregate counts so the
/// UI can report "removed N, M were already absent, K failed."
pub fn wipe_all() -> KeychainWipeResult {
    let mut result = KeychainWipeResult::default();
    for (service, account) in REGISTRY {
        match keyring::Entry::new(service, account) {
            Ok(entry) => match entry.delete_credential() {
                Ok(()) => result.removed += 1,
                Err(keyring::Error::NoEntry) => result.not_present += 1,
                Err(_) => result.failed += 1,
            },
            Err(_) => result.failed += 1,
        }
    }
    result
}

/// Snapshot of the registered entries, for tests and the QA contract.
pub fn registry_snapshot() -> Vec<(&'static str, &'static str)> {
    REGISTRY.to_vec()
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
    }
}
