// OS-level identity verification, used by the password recovery flow.
//
// On Windows, the UserConsentVerifier prompt is the same surface used for
// Windows Hello — it accepts the device biometric AND falls back to the
// account PIN/password, which matches Contract 02's "OS-level identity
// verification (device password / passkey)" requirement.
//
// On platforms where no equivalent prompt is wired up, this returns
// `IdentityVerificationUnavailable`. The recovery flow surfaces that as
// "recovery isn't available on this device — you must remember your password
// or reinstall", which is the conservative, no-data-exposed outcome the
// contract requires.

use crate::errors::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Verified,
    Declined,
}

pub fn is_available() -> bool {
    platform::is_available()
}

pub fn prompt(reason: &str) -> Result<Verdict, AppError> {
    platform::prompt(reason)
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use windows::core::HSTRING;
    use windows::Security::Credentials::UI::{
        UserConsentVerificationResult, UserConsentVerifier, UserConsentVerifierAvailability,
    };

    pub fn is_available() -> bool {
        matches!(
            UserConsentVerifier::CheckAvailabilityAsync().and_then(|op| op.get()),
            Ok(UserConsentVerifierAvailability::Available)
        )
    }

    pub fn prompt(reason: &str) -> Result<Verdict, AppError> {
        let message = HSTRING::from(reason);
        let result = UserConsentVerifier::RequestVerificationAsync(&message)
            .and_then(|op| op.get())
            .map_err(|e| AppError::IdentityVerification(format!("windows: {e}")))?;
        Ok(match result {
            UserConsentVerificationResult::Verified => Verdict::Verified,
            _ => Verdict::Declined,
        })
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use super::*;

    pub fn is_available() -> bool {
        false
    }

    pub fn prompt(_reason: &str) -> Result<Verdict, AppError> {
        Err(AppError::IdentityVerificationUnavailable)
    }
}
