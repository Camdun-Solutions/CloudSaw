// Integration tests for the auth module (Contract 03).
//
// These exercise the *real* public surface of `auth` end-to-end where that's
// possible without a live AWS account. Specifically:
//
//   - Profile discovery against synthetic `~/.aws/config` files (the SDK
//     and our parser both honor `AWS_CONFIG_FILE`, so we point it at a
//     sandbox file per test).
//   - Profile-name validation rejects shell metacharacters.
//   - `auth_test_profile` returns a structured failure (never a raw SDK
//     error) for missing profiles and unreachable endpoints.
//
// Tests are serialized through a module-level mutex because they share the
// `AWS_CONFIG_FILE` env var — running them in parallel would race.

use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use cloudsaw_lib::auth::{self, AuthError, ProfileSource, ProfileTestResult, TestFailureReason};
use cloudsaw_lib::errors::AppError;

fn env_lock() -> &'static Mutex<()> {
    static L: OnceLock<Mutex<()>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(()))
}

struct Sandbox {
    _guard: std::sync::MutexGuard<'static, ()>,
    dir: PathBuf,
    config_path: PathBuf,
    prev_config: Option<String>,
}

impl Sandbox {
    fn new(label: &str, config_body: &str) -> Self {
        let guard = env_lock().lock().unwrap_or_else(|p| p.into_inner());
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("cloudsaw-auth-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        let config_path = dir.join("config");
        fs::write(&config_path, config_body).unwrap();
        let prev_config = std::env::var("AWS_CONFIG_FILE").ok();
        std::env::set_var("AWS_CONFIG_FILE", &config_path);
        Self {
            _guard: guard,
            dir,
            config_path,
            prev_config,
        }
    }

    /// Variant that does NOT create a config file. Points `AWS_CONFIG_FILE`
    /// at a path that doesn't exist, so we can exercise the "no config"
    /// branch of `list_profiles`.
    fn new_without_config(label: &str) -> Self {
        let guard = env_lock().lock().unwrap_or_else(|p| p.into_inner());
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("cloudsaw-auth-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        let config_path = dir.join("does-not-exist");
        let prev_config = std::env::var("AWS_CONFIG_FILE").ok();
        std::env::set_var("AWS_CONFIG_FILE", &config_path);
        Self {
            _guard: guard,
            dir,
            config_path,
            prev_config,
        }
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        match &self.prev_config {
            Some(v) => std::env::set_var("AWS_CONFIG_FILE", v),
            None => std::env::remove_var("AWS_CONFIG_FILE"),
        }
        let _ = fs::remove_dir_all(&self.dir);
    }
}

// --- list_profiles -------------------------------------------------------

#[test]
fn list_profiles_returns_n_profiles_for_n_section_config() {
    let cfg = "[default]\nregion = us-east-1\n\n\
               [profile dev]\nregion = us-west-2\n\n\
               [profile prod]\nregion = eu-west-1\n";
    let _sb = Sandbox::new("listN", cfg);

    let profiles = auth::list_profiles().expect("list_profiles must succeed");
    let names: Vec<&str> = profiles.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, vec!["default", "dev", "prod"]);
    for p in &profiles {
        assert_eq!(p.source, ProfileSource::Cli);
    }
}

#[test]
fn list_profiles_flags_sso_profiles() {
    let cfg = "[profile work]\n\
               sso_session = main\n\
               sso_account_id = 111122223333\n\
               sso_role_name = Reader\n\n\
               [profile vanilla]\nregion = us-east-1\n";
    let _sb = Sandbox::new("sso", cfg);

    let profiles = auth::list_profiles().unwrap();
    let work = profiles.iter().find(|p| p.name == "work").unwrap();
    let vanilla = profiles.iter().find(|p| p.name == "vanilla").unwrap();
    assert_eq!(work.source, ProfileSource::Sso);
    assert_eq!(vanilla.source, ProfileSource::Cli);
}

#[test]
fn list_profiles_returns_empty_when_no_config_exists() {
    let _sb = Sandbox::new_without_config("nocfg");

    let profiles = auth::list_profiles().unwrap();
    assert!(profiles.is_empty(), "expected empty Vec, got {profiles:?}");
}

#[test]
fn list_profiles_does_not_open_credentials_file() {
    // Place a syntactically-broken `credentials` file next to the config.
    // If our code ever opened it, the AWS SDK / our parser would either
    // surface an error or silently include phantom profile names — both
    // would fail this test.
    let cfg = "[default]\nregion = us-east-1\n";
    let sb = Sandbox::new("creds-untouched", cfg);
    let creds = sb.config_path.parent().unwrap().join("credentials");
    fs::write(
        &creds,
        "[ghost]\naws_access_key_id = AKIAIOSFODNN7EXAMPLE\n",
    )
    .unwrap();

    let profiles = auth::list_profiles().unwrap();
    let names: Vec<&str> = profiles.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["default"],
        "credentials file must not contribute profile names"
    );
}

// --- Profile-name validation --------------------------------------------

#[tokio::test]
async fn test_profile_rejects_shell_metacharacters() {
    // No sandbox needed — validation happens before we touch the SDK.
    for bad in [
        "with space",
        "semi;colon",
        "pipe|chain",
        "$(whoami)",
        "../etc/passwd",
        "back`tick",
        "amp&erpand",
        "",
    ] {
        let err = auth::test_profile(bad)
            .await
            .err()
            .unwrap_or_else(|| panic!("expected error for profile name {bad:?}"));
        assert!(
            matches!(err, AuthError::InvalidProfileName),
            "expected InvalidProfileName for {bad:?}, got {err:?}"
        );
    }
}

#[tokio::test]
async fn get_caller_identity_rejects_shell_metacharacters() {
    let err = auth::get_caller_identity("$(rm -rf /)").await.unwrap_err();
    assert!(matches!(err, AuthError::InvalidProfileName));
}

// --- test_profile error categorization ----------------------------------

#[tokio::test]
async fn test_profile_missing_profile_returns_structured_failure() {
    // Config has profile "dev" but not "missing".
    let cfg = "[profile dev]\nregion = us-east-1\n";
    let _sb = Sandbox::new("missing", cfg);

    let result = auth::test_profile("missing").await.unwrap();
    match result {
        ProfileTestResult::Failure { reason, .. } => {
            assert!(
                matches!(reason, TestFailureReason::ProfileNotConfigured),
                "missing profile must classify as ProfileNotConfigured, got {reason:?}"
            );
        }
        ProfileTestResult::Success { .. } => {
            panic!("missing profile must not produce Success")
        }
    }
}

#[tokio::test]
async fn test_profile_completes_within_timeout_budget() {
    // Pin an SSO-style profile at an unreachable endpoint. Whatever the SDK
    // does internally, the wall-clock time must be bounded by our 10s
    // budget (Contract 03 §Constraints). We allow a small overhead margin
    // for runtime/teardown.
    let cfg = "[profile unreachable]\n\
               sso_start_url = https://unreachable.cloudsaw.test/start\n\
               sso_region = us-east-1\n\
               sso_account_id = 111122223333\n\
               sso_role_name = Reader\n";
    let _sb = Sandbox::new("timeout", cfg);

    let started = Instant::now();
    let result = auth::test_profile("unreachable").await;
    let elapsed = started.elapsed();

    // Must not exceed the budget by more than a small margin.
    assert!(
        elapsed < Duration::from_secs(15),
        "test_profile blew past timeout: {elapsed:?}"
    );

    // Must be a structured failure (no SSO token cached → expired/other).
    match result {
        Ok(ProfileTestResult::Failure { .. }) => {}
        Ok(ProfileTestResult::Success { .. }) => {
            panic!("unreachable endpoint must not produce Success")
        }
        Err(e) => panic!("unexpected hard error: {e:?}"),
    }
}

// --- AppError mapping (ensures stable IPC codes) -------------------------

#[test]
fn auth_error_maps_to_stable_app_error_codes() {
    fn assert_code(err: AuthError, expected: &str) {
        let mapped: AppError = err.into();
        assert_eq!(mapped.code(), expected);
    }
    assert_code(AuthError::ConfigUnreadable, "aws_config_unreadable");
    assert_code(AuthError::ProfileNotFound, "profile_not_found");
    assert_code(AuthError::InvalidProfileName, "invalid_input");
    assert_code(AuthError::Timeout, "aws_timeout");
    assert_code(AuthError::Connectivity, "aws_connectivity");
    assert_code(AuthError::SsoExpired, "aws_sso_expired");
    assert_code(
        AuthError::PermissionDenied("GetCallerIdentity"),
        "aws_permission_denied",
    );
}

#[test]
fn app_error_serializes_with_stable_code_and_message() {
    // The IPC boundary returns {code, message}. Each variant must serialize
    // with its own code — not collapsed to "internal_error".
    let err = AppError::AwsSsoExpired;
    let json = serde_json::to_value(&err).unwrap();
    assert_eq!(json["code"], "aws_sso_expired");
    assert_eq!(json["message"], "aws sso expired");

    let err = AppError::AwsPermissionDenied("GetCallerIdentity");
    let json = serde_json::to_value(&err).unwrap();
    assert_eq!(json["code"], "aws_permission_denied");
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("GetCallerIdentity"));
}

#[test]
fn app_error_does_not_leak_credential_patterns() {
    // No AppError variant emits text resembling an AWS access key. We
    // construct each user-facing variant and assert nothing in the
    // serialized output matches the AKIA prefix or a secret-key shape.
    let variants = [
        AppError::AwsConfigUnreadable,
        AppError::AwsProfileNotFound,
        AppError::AwsTimeout,
        AppError::AwsConnectivity,
        AppError::AwsSsoExpired,
        AppError::AwsPermissionDenied("GetCallerIdentity"),
    ];
    for v in &variants {
        let s = serde_json::to_string(v).unwrap();
        assert!(!s.contains("AKIA"), "code path may leak access key: {s}");
        // A secret key is 40 base64 chars; we don't want any error variant
        // accidentally embedding one.
        assert!(
            !regex_like_secret(&s),
            "code path may leak a secret-like token: {s}"
        );
    }
}

/// Cheap check for a 40-char base64 substring without pulling in a regex
/// dep. Walks windows of 40 chars and verifies they look base64-ish.
fn regex_like_secret(s: &str) -> bool {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() < 40 {
        return false;
    }
    for window in chars.windows(40) {
        if window.iter().all(|c| {
            c.is_ascii_alphanumeric() || *c == '+' || *c == '/' || *c == '='
        }) {
            return true;
        }
    }
    false
}
