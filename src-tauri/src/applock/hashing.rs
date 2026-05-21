// Argon2id password hashing for the app lock.
//
// Why Argon2id: the algorithm the contract calls out, and what OWASP currently
// recommends for interactive logins. We use the PHC string output format so
// the stored hash carries its own algorithm + parameters and can be verified
// without separately tracking which params were used at hash time.
//
// The password material is held in a `Zeroizing<String>` end-to-end so memory
// is wiped as soon as the function frame drops. We never log the password,
// never store it, never pass it across IPC except as the inbound request.

use argon2::{Algorithm, Argon2, Params, Version};
use password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use zeroize::Zeroizing;

use crate::errors::AppError;

/// OWASP-recommended Argon2id parameters for interactive login (May 2026
/// guidance): 19 MiB memory, t=2, p=1, 32-byte tag. Bumping these is safe;
/// stored hashes embed the params they were created with so verification will
/// keep working across changes.
const MEMORY_KIB: u32 = 19_456;
const ITERATIONS: u32 = 2;
const PARALLELISM: u32 = 1;
const OUTPUT_LEN: usize = 32;

fn hasher() -> Result<Argon2<'static>, AppError> {
    let params = Params::new(MEMORY_KIB, ITERATIONS, PARALLELISM, Some(OUTPUT_LEN))
        .map_err(|e| AppError::Hash(format!("params: {e}")))?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}

/// Hash `password` and return its PHC-format string. Salt is freshly generated
/// per call from the OS CSPRNG.
pub fn hash_password(password: &Zeroizing<String>) -> Result<String, AppError> {
    let salt = SaltString::generate(&mut OsRng);
    let argon = hasher()?;
    let phc = argon
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| AppError::Hash(format!("hash: {e}")))?
        .to_string();
    Ok(phc)
}

/// Verify `password` against a previously stored PHC string. Returns
/// `Ok(true)` on match, `Ok(false)` on mismatch. A malformed stored hash
/// surfaces a typed error rather than silently failing as "no match" — that
/// would mask data corruption.
pub fn verify_password(stored_phc: &str, password: &Zeroizing<String>) -> Result<bool, AppError> {
    let parsed = PasswordHash::new(stored_phc)
        .map_err(|e| AppError::Hash(format!("parse stored hash: {e}")))?;
    let argon = hasher()?;
    match argon.verify_password(password.as_bytes(), &parsed) {
        Ok(()) => Ok(true),
        Err(password_hash::Error::Password) => Ok(false),
        Err(e) => Err(AppError::Hash(format!("verify: {e}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_matches() {
        let pw = Zeroizing::new("correct horse battery staple".to_string());
        let phc = hash_password(&pw).unwrap();
        assert!(verify_password(&phc, &pw).unwrap());
    }

    #[test]
    fn wrong_password_does_not_match() {
        let pw = Zeroizing::new("right".to_string());
        let bad = Zeroizing::new("wrong".to_string());
        let phc = hash_password(&pw).unwrap();
        assert!(!verify_password(&phc, &bad).unwrap());
    }

    #[test]
    fn distinct_salt_per_hash() {
        let pw = Zeroizing::new("same input".to_string());
        let a = hash_password(&pw).unwrap();
        let b = hash_password(&pw).unwrap();
        assert_ne!(a, b, "every hash must use a fresh salt");
    }

    #[test]
    fn malformed_stored_hash_is_a_typed_error() {
        let pw = Zeroizing::new("x".to_string());
        let result = verify_password("not-a-phc-string", &pw);
        assert!(matches!(result, Err(AppError::Hash(_))));
    }
}
