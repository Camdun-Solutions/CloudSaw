// Production `CredentialStore` — wraps the `keyring` crate so
// secrets land in the OS-native keychain (macOS Keychain, Windows
// Credential Manager, Linux Secret Service). This is the ONLY
// module in CloudSaw that may reference `keyring::*` after
// Contract 17. The Contract 17 QA asserts that invariant.
//
// Behavior is unchanged from the pre-Contract-17 free functions:
// the same entry names, the same backend selection, the same
// "missing entry == Ok(None) / Ok(false)" semantics. End users
// see no difference.

use super::{CredentialStore, KeychainError};

#[derive(Debug, Default)]
pub struct KeyringStore;

impl KeyringStore {
    pub fn new() -> Self {
        Self
    }
}

impl CredentialStore for KeyringStore {
    fn get(&self, service: &str, account: &str) -> Result<Option<String>, KeychainError> {
        match keyring::Entry::new(service, account)
            .map_err(|e| KeychainError::Backend(e.to_string()))?
            .get_password()
        {
            Ok(s) => Ok(Some(s)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(KeychainError::Backend(e.to_string())),
        }
    }

    fn set(&self, service: &str, account: &str, secret: &str) -> Result<(), KeychainError> {
        keyring::Entry::new(service, account)
            .map_err(|e| KeychainError::Backend(e.to_string()))?
            .set_password(secret)
            .map_err(|e| KeychainError::Backend(e.to_string()))
    }

    fn delete(&self, service: &str, account: &str) -> Result<bool, KeychainError> {
        match keyring::Entry::new(service, account)
            .map_err(|e| KeychainError::Backend(e.to_string()))?
            .delete_credential()
        {
            Ok(()) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(e) => Err(KeychainError::Backend(e.to_string())),
        }
    }
}
