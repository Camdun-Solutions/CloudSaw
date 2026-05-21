// Platform biometric unlock.
//
// "Biometric" here means whatever the OS calls user-presence verification
// scoped to the active account: Windows Hello (face/fingerprint/PIN),
// Touch ID on macOS, etc. We rely entirely on the OS prompt's pass/fail
// result and store no biometric secret on our side — the OS is the source of
// truth.
//
// Per Contract 02: biometric MUST be opt-in and disableable, MUST NOT be the
// only unlock path (password always works), and the option MUST be hidden /
// disabled when hardware is absent.

use crate::errors::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Availability {
    /// Hardware present and configured. Biometric prompt can be invoked.
    Available,
    /// Hardware present but the OS reports it as not currently usable
    /// (disabled, not enrolled, busy). UI should explain rather than offer.
    Unconfigured,
    /// No biometric path on this platform/host. UI should hide the option.
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Verified,
    Declined,
}

/// Probe whether the platform exposes a biometric/user-presence prompt this
/// session. Cheap and side-effect free.
pub fn availability() -> Availability {
    platform::availability()
}

/// Show the OS biometric prompt with `reason` as the user-visible message.
/// Returns `Verified` only when the OS confirms user presence; any other
/// outcome (cancelled, locked out, hardware error) maps to `Declined` so the
/// app surface never tries to distinguish them.
pub fn prompt(reason: &str) -> Result<Verdict, AppError> {
    platform::prompt(reason)
}

// --- Per-platform implementations ----------------------------------------

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use windows::core::HSTRING;
    use windows::Security::Credentials::UI::{
        UserConsentVerificationResult, UserConsentVerifier, UserConsentVerifierAvailability,
    };

    pub fn availability() -> Availability {
        match UserConsentVerifier::CheckAvailabilityAsync().and_then(|op| op.get()) {
            Ok(UserConsentVerifierAvailability::Available) => Availability::Available,
            Ok(UserConsentVerifierAvailability::DeviceNotPresent)
            | Ok(UserConsentVerifierAvailability::NotConfiguredForUser)
            | Ok(UserConsentVerifierAvailability::DisabledByPolicy)
            | Ok(UserConsentVerifierAvailability::DeviceBusy) => Availability::Unconfigured,
            _ => Availability::Unavailable,
        }
    }

    pub fn prompt(reason: &str) -> Result<Verdict, AppError> {
        let message = HSTRING::from(reason);
        let result = UserConsentVerifier::RequestVerificationAsync(&message)
            .and_then(|op| op.get())
            .map_err(|e| AppError::Biometric(format!("windows hello: {e}")))?;
        Ok(match result {
            UserConsentVerificationResult::Verified => Verdict::Verified,
            _ => Verdict::Declined,
        })
    }
}

// macOS and Linux: structural shim returning Unavailable. A real LocalAuth /
// PAM implementation drops in here without touching callers. Until it does,
// the UI hides the biometric option on these platforms and password unlock
// works as the (only) path.
#[cfg(not(target_os = "windows"))]
mod platform {
    use super::*;

    pub fn availability() -> Availability {
        Availability::Unavailable
    }

    pub fn prompt(_reason: &str) -> Result<Verdict, AppError> {
        Err(AppError::BiometricUnavailable)
    }
}
