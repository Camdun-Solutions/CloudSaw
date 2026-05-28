// STS calls used by the auth module.
//
// Two surfaces:
//
//   get_caller_identity(profile) — returns a typed `CallerIdentity` or an
//     `AuthError`. Internal callers (later contracts) use this when they
//     want errors as `Result`.
//
//   test_profile(profile) — returns a `ProfileTestResult` that flattens
//     "did it work?" into one shape for the UI. Both branches are stable,
//     enumerated, and never carry credential material or full ARNs.
//
// Constraints enforced here:
//   - Bounded ~10s operation timeout (both via SDK config and an outer
//     tokio::time::timeout, since credential resolution can hang in ways
//     the SDK's per-operation timeout doesn't fully cover).
//   - The SDK credential provider chain is used unmodified — we only set
//     `profile_name`.
//   - Profile name is validated before any SDK call (defense in depth even
//     though no shell is invoked).
//   - SDK errors are categorized into stable `AuthError` variants; the raw
//     error chain never crosses the IPC boundary.

use std::time::Duration;

use aws_config::timeout::TimeoutConfig;
use aws_config::BehaviorVersion;
use aws_sdk_sts::operation::get_caller_identity::GetCallerIdentityError;
use aws_smithy_runtime_api::client::orchestrator::HttpResponse;
use aws_smithy_runtime_api::client::result::SdkError;
use serde::Serialize;

use super::error::AuthError;
use super::profiles::{is_valid_profile_name, list_profiles};

/// Total wall-clock budget for `test_profile` and `get_caller_identity`.
/// The contract pins this at ~10s.
const STS_CALL_BUDGET: Duration = Duration::from_secs(10);

/// What STS told us about who the caller is.
///
/// All three fields are populated on success. Per CLAUDE.md §4.4, logs MUST
/// redact account IDs and truncate ARNs — but the values returned across IPC
/// here are intentionally full so the UI can show the user "you're
/// authenticated as <full ARN>". The user already owns this data; the
/// concern is leakage through logs/error surfaces, not the UI itself.
#[derive(Debug, Clone, Serialize)]
pub struct CallerIdentity {
    pub account_id: String,
    pub user_id: String,
    pub arn: String,
}

/// Reason for a failed `test_profile`. Enumerated so the UI maps to
/// localized copy; never carries free-form SDK messages.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TestFailureReason {
    /// `~/.aws/config` couldn't be read or the requested profile is missing.
    ProfileNotConfigured,
    /// SSO access token is expired — user must `aws sso login` (or use the
    /// onboarding wizard in a later contract).
    SsoExpired,
    /// `sts:GetCallerIdentity` returned AccessDenied.
    PermissionDenied,
    /// Network unreachable / TLS handshake failed.
    Connectivity,
    /// Call exceeded the 10s budget.
    Timeout,
    /// Anything else — kept terse so we never leak credential text.
    Other,
}

/// Tagged union returned by `test_profile`. The `status` discriminator
/// is a deliberate API choice — the UI switches on it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum ProfileTestResult {
    Success {
        identity: CallerIdentity,
    },
    Failure {
        reason: TestFailureReason,
        /// The failing API name, e.g. "GetCallerIdentity". Constant string,
        /// never a value derived from the SDK error.
        api: Option<&'static str>,
    },
}

pub async fn get_caller_identity(profile: &str) -> Result<CallerIdentity, AuthError> {
    if !is_valid_profile_name(profile) {
        return Err(AuthError::InvalidProfileName);
    }
    // Pre-check: if the profile isn't in ~/.aws/config, surface a clear
    // ProfileNotFound rather than letting the SDK fall through to IMDS and
    // mis-classify the failure as a network/connectivity error. (`list_profiles`
    // returns an empty Vec when no config file exists; in that case we let the
    // SDK proceed — environment-only credentials are still a valid path.)
    let known = list_profiles().unwrap_or_default();
    if !known.is_empty() && !known.iter().any(|p| p.name == profile) {
        return Err(AuthError::ProfileNotFound);
    }
    let owned = profile.to_string();
    let fut = async move {
        let cfg = aws_config::defaults(BehaviorVersion::latest())
            .profile_name(&owned)
            .timeout_config(
                TimeoutConfig::builder()
                    .operation_timeout(STS_CALL_BUDGET)
                    .operation_attempt_timeout(STS_CALL_BUDGET)
                    .build(),
            )
            .load()
            .await;
        let client = aws_sdk_sts::Client::new(&cfg);
        client.get_caller_identity().send().await
    };

    let resp = match tokio::time::timeout(STS_CALL_BUDGET, fut).await {
        Ok(Ok(resp)) => resp,
        Ok(Err(err)) => return Err(classify(&err)),
        Err(_elapsed) => return Err(AuthError::Timeout),
    };

    let account_id = resp
        .account()
        .ok_or(AuthError::Internal("sts_account_missing"))?
        .to_string();
    let user_id = resp
        .user_id()
        .ok_or(AuthError::Internal("sts_user_id_missing"))?
        .to_string();
    let arn = resp
        .arn()
        .ok_or(AuthError::Internal("sts_arn_missing"))?
        .to_string();

    Ok(CallerIdentity {
        account_id,
        user_id,
        arn,
    })
}

pub async fn test_profile(profile: &str) -> Result<ProfileTestResult, AuthError> {
    match get_caller_identity(profile).await {
        Ok(identity) => Ok(ProfileTestResult::Success { identity }),
        Err(AuthError::InvalidProfileName) => Err(AuthError::InvalidProfileName),
        Err(AuthError::ProfileNotFound) => Ok(ProfileTestResult::Failure {
            reason: TestFailureReason::ProfileNotConfigured,
            api: None,
        }),
        Err(AuthError::SsoExpired) => Ok(ProfileTestResult::Failure {
            reason: TestFailureReason::SsoExpired,
            api: Some("GetCallerIdentity"),
        }),
        Err(AuthError::PermissionDenied(api)) => Ok(ProfileTestResult::Failure {
            reason: TestFailureReason::PermissionDenied,
            api: Some(api),
        }),
        Err(AuthError::Connectivity) => Ok(ProfileTestResult::Failure {
            reason: TestFailureReason::Connectivity,
            api: Some("GetCallerIdentity"),
        }),
        Err(AuthError::Timeout) => Ok(ProfileTestResult::Failure {
            reason: TestFailureReason::Timeout,
            api: Some("GetCallerIdentity"),
        }),
        Err(AuthError::ConfigUnreadable)
        | Err(AuthError::Internal(_))
        // PR #66: these variants only arise from create_profile and
        // never from get_caller_identity, but the match must be
        // exhaustive — bucket them with the generic "Other" failure
        // so a future call-chain accident surfaces gracefully.
        | Err(AuthError::DuplicateProfileName)
        | Err(AuthError::ConfigWriteFailed(_)) => Ok(ProfileTestResult::Failure {
            reason: TestFailureReason::Other,
            api: Some("GetCallerIdentity"),
        }),
    }
}

/// Map an SDK error into a stable `AuthError`. We walk the error source
/// chain stringifying each layer, then keyword-match — this is the
/// pragmatic way to recognize SSO-expiry and credential errors, because
/// the SDK doesn't expose them as separate enum variants at this layer.
///
/// Critically, the joined text is consumed *only* for classification. No
/// part of it is ever returned to the caller or logged.
fn classify(err: &SdkError<GetCallerIdentityError, HttpResponse>) -> AuthError {
    match err {
        SdkError::TimeoutError(_) => AuthError::Timeout,
        SdkError::DispatchFailure(_) => AuthError::Connectivity,
        SdkError::ServiceError(srv) => {
            let svc = srv.err();
            // The STS service error metadata exposes the error code; we use
            // it for precise classification rather than string-matching.
            let code = svc
                .meta()
                .code()
                .map(str::to_ascii_lowercase)
                .unwrap_or_default();
            match code.as_str() {
                "expiredtoken" | "expiredtokenexception" => AuthError::SsoExpired,
                "accessdenied"
                | "accessdeniedexception"
                | "unauthorizedoperation"
                | "invalidclienttokenid" => AuthError::PermissionDenied("GetCallerIdentity"),
                _ => AuthError::Internal("sts_service"),
            }
        }
        SdkError::ConstructionFailure(_) => classify_construction(err),
        SdkError::ResponseError(_) => AuthError::Internal("sts_response"),
        _ => AuthError::Internal("sts_unknown"),
    }
}

/// ConstructionFailure most commonly means credential resolution failed
/// before the request was even built. The two cases we care about are:
///
///   * SSO token cache missing / expired → AuthError::SsoExpired
///   * Profile doesn't exist             → AuthError::ProfileNotFound
///
/// We classify by walking the error source chain and looking for
/// well-known phrases. The strings are stable across SDK versions for the
/// case we care about, but we treat anything we don't recognize as a
/// generic "config unreadable" rather than guessing.
fn classify_construction(err: &(dyn std::error::Error + 'static)) -> AuthError {
    let joined = flatten_chain(err);
    let lower = joined.to_ascii_lowercase();
    if lower.contains("expired")
        || lower.contains("sso session")
        || lower.contains("token has expired")
        || lower.contains("forbidden")
    {
        AuthError::SsoExpired
    } else if lower.contains("profile") && lower.contains("not found") {
        AuthError::ProfileNotFound
    } else if lower.contains("dispatch") || lower.contains("connect") {
        AuthError::Connectivity
    } else {
        AuthError::ConfigUnreadable
    }
}

fn flatten_chain(err: &(dyn std::error::Error + 'static)) -> String {
    let mut out = String::new();
    let mut cur: Option<&(dyn std::error::Error + 'static)> = Some(err);
    while let Some(e) = cur {
        if !out.is_empty() {
            out.push_str(" | ");
        }
        out.push_str(&e.to_string());
        cur = e.source();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flatten_chain_concatenates_sources() {
        #[derive(Debug)]
        struct Inner;
        impl std::fmt::Display for Inner {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "inner: token has expired")
            }
        }
        impl std::error::Error for Inner {}

        #[derive(Debug)]
        struct Outer(Inner);
        impl std::fmt::Display for Outer {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "outer: failed to construct request")
            }
        }
        impl std::error::Error for Outer {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                Some(&self.0)
            }
        }

        let e = Outer(Inner);
        let flat = flatten_chain(&e);
        assert!(flat.contains("outer"));
        assert!(flat.contains("inner"));
        assert!(matches!(classify_construction(&e), AuthError::SsoExpired));
    }
}
