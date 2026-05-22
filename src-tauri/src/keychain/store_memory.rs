// In-memory `CredentialStore` — test only.
//
// Backed by a `RwLock<HashMap<(String, String), String>>`. No
// `keyring` dependency, no D-Bus, no desktop session, no OS keychain
// interaction at all. Lifetime is bounded by the test process; no
// state persists to disk.
//
// Contract 17 §Constraints: "The in-memory implementation MUST NOT
// be reachable from the running application." The module is `pub`
// only so the integration tests can `use cloudsaw_lib::keychain::
// InMemoryStore`; nothing in feature code (eventlog / github / ai /
// wipe) references it.

use std::collections::HashMap;
use std::sync::RwLock;

use super::{CredentialStore, KeychainError};

#[derive(Debug, Default)]
pub struct InMemoryStore {
    inner: RwLock<HashMap<(String, String), String>>,
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Drop every entry. Tests call this between cases so each one
    /// runs against an empty store. The `install_in_memory_for_tests`
    /// helper invokes it automatically.
    pub fn clear(&self) {
        self.inner
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
    }

    /// Count of entries currently held. Convenience for QA assertions
    /// like "after panic-wipe the store is empty."
    pub fn len(&self) -> usize {
        self.inner.read().map(|g| g.len()).unwrap_or(0)
    }

    /// Whether the store is empty. Used in qa17_test.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl CredentialStore for InMemoryStore {
    fn get(&self, service: &str, account: &str) -> Result<Option<String>, KeychainError> {
        let guard = self
            .inner
            .read()
            .map_err(|p| KeychainError::Backend(format!("in-memory store poisoned: {p}")))?;
        Ok(guard
            .get(&(service.to_string(), account.to_string()))
            .cloned())
    }

    fn set(&self, service: &str, account: &str, secret: &str) -> Result<(), KeychainError> {
        let mut guard = self
            .inner
            .write()
            .map_err(|p| KeychainError::Backend(format!("in-memory store poisoned: {p}")))?;
        guard.insert(
            (service.to_string(), account.to_string()),
            secret.to_string(),
        );
        Ok(())
    }

    fn delete(&self, service: &str, account: &str) -> Result<bool, KeychainError> {
        let mut guard = self
            .inner
            .write()
            .map_err(|p| KeychainError::Backend(format!("in-memory store poisoned: {p}")))?;
        Ok(guard
            .remove(&(service.to_string(), account.to_string()))
            .is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_store_get_returns_none() {
        let s = InMemoryStore::new();
        assert!(matches!(s.get("svc", "acct"), Ok(None)));
    }

    #[test]
    fn set_then_get_round_trips() {
        let s = InMemoryStore::new();
        s.set("svc", "acct", "secret-value").unwrap();
        assert_eq!(
            s.get("svc", "acct").unwrap().as_deref(),
            Some("secret-value")
        );
    }

    #[test]
    fn delete_returns_true_when_present_false_when_absent() {
        let s = InMemoryStore::new();
        assert!(matches!(s.delete("svc", "acct"), Ok(false)));
        s.set("svc", "acct", "x").unwrap();
        assert!(matches!(s.delete("svc", "acct"), Ok(true)));
        // Second delete: already absent. Same Ok(false) shape.
        assert!(matches!(s.delete("svc", "acct"), Ok(false)));
    }

    #[test]
    fn clear_empties_everything() {
        let s = InMemoryStore::new();
        s.set("a", "b", "c").unwrap();
        s.set("d", "e", "f").unwrap();
        assert_eq!(s.len(), 2);
        s.clear();
        assert!(s.is_empty());
    }
}
