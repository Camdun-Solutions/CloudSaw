// AWS authentication — Contract 03.
//
// Public surface (mirrors the contract's "Expected Output"):
//
//     list_profiles()                      -> Vec<ProfileInfo>
//     get_caller_identity(profile)         -> CallerIdentity
//     test_profile(profile)                -> ProfileTestResult
//
// All three return `Result<T, AuthError>`. Panics in production paths fail
// the build (Cargo.toml [profile.release] panic = "abort"; `unwrap`/`expect`
// avoided here).
//
// What this module deliberately does NOT do:
//   - Read ~/.aws/credentials. The SDK provider chain reads it implicitly,
//     but CloudSaw code never opens it (see CLAUDE.md §4.3 and Contract 03
//     Constraints).
//   - Accept long-term access keys from the UI. The IPC surface has no path
//     to do so by construction (no command takes credential material).
//   - Cache STS responses. Every call is fresh; Contract 03 §Constraints.
//   - Hold any AWS SDK type. The structs returned across IPC are plain
//     serializable data; SDK types stay inside `sts.rs`.

pub mod error;
mod profiles;
mod sts;

pub use error::AuthError;
pub use profiles::{ProfileInfo, ProfileSource};
pub use sts::{CallerIdentity, ProfileTestResult, TestFailureReason};

/// Discover profile names from `~/.aws/config`. Honors `AWS_CONFIG_FILE`.
/// Returns an empty list (not an error) if no config file exists.
pub fn list_profiles() -> Result<Vec<ProfileInfo>, AuthError> {
    profiles::list_profiles()
}

/// Resolve credentials for `profile` via the SDK provider chain and call
/// `sts:GetCallerIdentity` with a bounded 10s budget.
pub async fn get_caller_identity(profile: &str) -> Result<CallerIdentity, AuthError> {
    sts::get_caller_identity(profile).await
}

/// Friendlier sibling of `get_caller_identity`: known failure modes (SSO
/// expired, permission denied, connectivity, timeout, missing profile) are
/// folded into the `ProfileTestResult` so the UI renders one shape. Only
/// programmer errors (invalid profile name) escape as `Err`.
pub async fn test_profile(profile: &str) -> Result<ProfileTestResult, AuthError> {
    sts::test_profile(profile).await
}
