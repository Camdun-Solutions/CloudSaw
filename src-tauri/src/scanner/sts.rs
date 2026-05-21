// Scanner-role `sts:AssumeRole` call.
//
// Contract 06 §Constraints:
//   * Each scan MUST call `sts:AssumeRole` fresh against the account's
//     `CloudSawScannerRole`. STS session credentials MUST NOT be cached.
//   * Session duration ≤ 3600 seconds; `RoleSessionName` identifies the scan.
//   * The trust-policy `ExternalId` is supplied as the confused-deputy guard.
//
// CLAUDE.md §4.3: temporary STS credentials (≤1 hour) are used for all
// scanning. No long-lived credentials are cached. This module returns the
// credentials by value to the orchestrator, which immediately hands them off
// to the child process's environment and drops the local copy.
//
// `AssumedCredentials` derives `ZeroizeOnDrop` so the buffer is overwritten
// when the orchestrator's binding goes out of scope. We pass the credentials
// to the child via `Command::env`, which copies the value into a new heap
// allocation owned by the std-library — the original Zeroizing buffer is
// what `ZeroizeOnDrop` handles. (The copy inside the child is its own
// problem; AWS SDKs and ScoutSuite manage their own credential lifetime.)

use std::time::Duration;

use aws_config::timeout::TimeoutConfig;
use aws_config::BehaviorVersion;
use aws_sdk_sts::operation::assume_role::AssumeRoleError;
use aws_smithy_runtime_api::client::orchestrator::HttpResponse;
use aws_smithy_runtime_api::client::result::SdkError;
use zeroize::Zeroize;

use super::error::ScannerError;

/// Total wall-clock budget for AssumeRole. AWS typically responds in well
/// under 10s; we cap aggressively so a hung credential resolver can't pin
/// the scan in the `assuming_role` state indefinitely.
const ASSUME_ROLE_BUDGET: Duration = Duration::from_secs(15);

/// Session duration handed to AssumeRole. Capped at the contract's 3600s
/// ceiling. ScoutSuite scans typically finish well inside this window;
/// CloudTrail records the actual session lifetime regardless.
pub const SCAN_SESSION_DURATION_SECONDS: i32 = 3600;

/// Temporary credentials the orchestrator places into the ScoutSuite child's
/// environment. Drops zero the buffers so a panic inside the orchestrator
/// can't leave credential bytes hanging around in process memory.
///
/// We implement Drop manually (rather than `#[derive(ZeroizeOnDrop)]`) so
/// the Debug impl below has somewhere to point — and because the zeroize
/// trait set we depend on already has Zeroize for String.
pub struct AssumedCredentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: String,
}

impl std::fmt::Debug for AssumedCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never include the actual credential material in Debug output. The
        // only thing safe to surface is "yes, it's an AssumedCredentials".
        f.debug_struct("AssumedCredentials")
            .field("access_key_id", &"<redacted>")
            .field("secret_access_key", &"<redacted>")
            .field("session_token", &"<redacted>")
            .finish()
    }
}

impl Drop for AssumedCredentials {
    fn drop(&mut self) {
        self.access_key_id.zeroize();
        self.secret_access_key.zeroize();
        self.session_token.zeroize();
    }
}

/// Perform `sts:AssumeRole` against the scanner role attached to the account.
///
/// Test seam: if `CLOUDSAW_SCANNER_STUB_STS=1`, the function short-circuits
/// with hardcoded fake credentials. This is the same pattern as the rest of
/// the project's binary-override seams — useful for integration tests that
/// drive the orchestrator end-to-end without real AWS.
pub async fn assume_scanner_role(
    profile: &str,
    role_arn: &str,
    external_id: &str,
    role_session_name: &str,
) -> Result<AssumedCredentials, ScannerError> {
    if std::env::var_os("CLOUDSAW_SCANNER_STUB_STS").is_some() {
        return Ok(stub_credentials());
    }

    let cfg = aws_config::defaults(BehaviorVersion::latest())
        .profile_name(profile)
        .timeout_config(
            TimeoutConfig::builder()
                .operation_timeout(ASSUME_ROLE_BUDGET)
                .operation_attempt_timeout(ASSUME_ROLE_BUDGET)
                .build(),
        )
        .load()
        .await;
    let client = aws_sdk_sts::Client::new(&cfg);

    let fut = client
        .assume_role()
        .role_arn(role_arn)
        .role_session_name(role_session_name)
        .external_id(external_id)
        .duration_seconds(SCAN_SESSION_DURATION_SECONDS)
        .send();

    let resp = match tokio::time::timeout(ASSUME_ROLE_BUDGET, fut).await {
        Ok(Ok(r)) => r,
        Ok(Err(err)) => return Err(classify(&err)),
        Err(_) => return Err(ScannerError::AssumeRoleFailed("timeout")),
    };

    let creds = resp
        .credentials()
        .ok_or(ScannerError::AssumeRoleFailed("missing_credentials"))?;

    Ok(AssumedCredentials {
        access_key_id: creds.access_key_id().to_string(),
        secret_access_key: creds.secret_access_key().to_string(),
        session_token: creds.session_token().to_string(),
    })
}

fn stub_credentials() -> AssumedCredentials {
    AssumedCredentials {
        access_key_id: "ASIASTUBSTUBSTUBSTUB".to_string(),
        secret_access_key: "STUB-SECRET-KEY-DO-NOT-USE".to_string(),
        session_token: "STUB-SESSION-TOKEN-DO-NOT-USE".to_string(),
    }
}

/// Map an SDK error into a stable scanner error tag. The joined text is
/// consumed *only* for classification and is never returned or logged.
fn classify(err: &SdkError<AssumeRoleError, HttpResponse>) -> ScannerError {
    match err {
        SdkError::TimeoutError(_) => ScannerError::AssumeRoleFailed("timeout"),
        SdkError::DispatchFailure(_) => ScannerError::AssumeRoleFailed("connectivity"),
        SdkError::ServiceError(srv) => {
            let svc = srv.err();
            let code = svc
                .meta()
                .code()
                .map(str::to_ascii_lowercase)
                .unwrap_or_default();
            match code.as_str() {
                "expiredtoken" | "expiredtokenexception" => {
                    ScannerError::AssumeRoleFailed("expired")
                }
                "accessdenied"
                | "accessdeniedexception"
                | "unauthorizedoperation"
                | "invalidclienttokenid" => ScannerError::AssumeRoleFailed("access_denied"),
                _ => ScannerError::AssumeRoleFailed("service"),
            }
        }
        SdkError::ConstructionFailure(_) => ScannerError::AssumeRoleFailed("construction"),
        SdkError::ResponseError(_) => ScannerError::AssumeRoleFailed("response"),
        _ => ScannerError::AssumeRoleFailed("unknown"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_credentials_have_expected_shape() {
        let c = stub_credentials();
        assert!(c.access_key_id.starts_with("ASIA"));
        assert!(!c.secret_access_key.is_empty());
        assert!(!c.session_token.is_empty());
    }

    #[test]
    fn debug_output_does_not_leak_secret_bytes() {
        let c = AssumedCredentials {
            access_key_id: "AKIAREAL".into(),
            secret_access_key: "very-secret".into(),
            session_token: "session-bytes".into(),
        };
        let formatted = format!("{:?}", c);
        assert!(!formatted.contains("AKIAREAL"));
        assert!(!formatted.contains("very-secret"));
        assert!(!formatted.contains("session-bytes"));
        assert!(formatted.contains("<redacted>"));
    }
}
