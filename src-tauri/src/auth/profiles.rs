// Profile discovery.
//
// The contract is strict: profile discovery uses `~/.aws/config` parsing
// only. `~/.aws/credentials` is never opened by CloudSaw code, even though
// it's a valid place for the AWS CLI to define profiles — long-term secret
// access keys live there, and CloudSaw refuses to read them.
//
// We hand-roll a minimal INI parser (no third-party dep) because the AWS
// config grammar we care about is a tiny subset:
//
//   [default]            -> a profile named "default"
//   [profile NAME]       -> a profile named NAME
//   [sso-session NAME]   -> NOT a profile (skipped)
//   [services NAME]      -> NOT a profile (skipped)
//
// Keys inside a profile section flag it as SSO when they reference an SSO
// session or start URL. That informs the UI badge but does not affect
// authentication — the SDK provider chain decides how to resolve creds.

use std::fs;
use std::path::PathBuf;

use serde::Serialize;

use super::error::AuthError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileSource {
    /// A standard AWS CLI profile (static creds via ~/.aws/credentials, or
    /// `credential_process`, or IMDS, etc.).
    Cli,
    /// Profile is backed by IAM Identity Center / `sso_session` or has a
    /// direct `sso_start_url`. May require `aws sso login` to refresh.
    Sso,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProfileInfo {
    pub name: String,
    pub source: ProfileSource,
}

/// Returns the path the SDK would resolve as the config file. Honors
/// `AWS_CONFIG_FILE` (matches the SDK's own behavior) so tests can point
/// at a sandbox file. Returns `None` if no home dir is resolvable AND no
/// override is set — in that case `list_profiles` returns an empty list,
/// not an error.
pub fn config_path() -> Option<PathBuf> {
    if let Ok(override_path) = std::env::var("AWS_CONFIG_FILE") {
        if !override_path.is_empty() {
            return Some(PathBuf::from(override_path));
        }
    }
    dirs::home_dir().map(|h| h.join(".aws").join("config"))
}

pub fn list_profiles() -> Result<Vec<ProfileInfo>, AuthError> {
    let Some(path) = config_path() else {
        return Ok(Vec::new());
    };
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(AuthError::ConfigUnreadable),
    };
    Ok(parse(&content))
}

/// True iff `name` is a syntactically safe AWS profile name. The SDK itself
/// doesn't enforce this, but CloudSaw rejects anything outside
/// `[A-Za-z0-9_.\-]{1,128}` so the value cannot contain shell metacharacters
/// or path separators. We never invoke a shell — this is defense in depth
/// for the frontend boundary (see CLAUDE.md §4.1).
pub fn is_valid_profile_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 128 {
        return false;
    }
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn parse(content: &str) -> Vec<ProfileInfo> {
    let mut out: Vec<ProfileInfo> = Vec::new();
    // (index_into_out, is_profile_section)
    let mut current: Option<(usize, bool)> = None;

    for raw in content.lines() {
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if let Some(section) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            let section = section.trim();
            let name = profile_name_from_section(section);
            current = match name {
                Some(n) => {
                    out.push(ProfileInfo {
                        name: n,
                        source: ProfileSource::Cli,
                    });
                    Some((out.len() - 1, true))
                }
                None => Some((0, false)),
            };
            continue;
        }
        if let Some((idx, true)) = current {
            if let Some(eq) = line.find('=') {
                let key = line[..eq].trim();
                if key == "sso_session" || key == "sso_start_url" {
                    if let Some(p) = out.get_mut(idx) {
                        p.source = ProfileSource::Sso;
                    }
                }
            }
        }
    }
    out
}

fn profile_name_from_section(section: &str) -> Option<String> {
    if section == "default" {
        return Some("default".to_string());
    }
    if let Some(rest) = section.strip_prefix("profile ") {
        let name = rest.trim();
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }
    None
}

fn strip_comment(line: &str) -> &str {
    // INI comments start with `#` or `;` at the *start* of the line. Keys may
    // legitimately contain those characters mid-value (rare, but tolerated),
    // so we only strip when the line begins with a comment char after
    // whitespace.
    let trimmed_start = line.trim_start();
    if trimmed_start.starts_with('#') || trimmed_start.starts_with(';') {
        return "";
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_yields_no_profiles() {
        assert!(parse("").is_empty());
    }

    #[test]
    fn parses_default_and_named_profiles() {
        let cfg = r#"
[default]
region = us-east-1

[profile dev]
region = us-west-2

[profile prod]
role_arn = arn:aws:iam::111122223333:role/Foo
source_profile = dev
"#;
        let profiles = parse(cfg);
        let names: Vec<&str> = profiles.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["default", "dev", "prod"]);
        for p in &profiles {
            assert_eq!(p.source, ProfileSource::Cli);
        }
    }

    #[test]
    fn detects_sso_via_session_or_start_url() {
        let cfg = r#"
[profile sso-via-session]
sso_session = main
sso_account_id = 111122223333
sso_role_name = Reader

[profile sso-via-start-url]
sso_start_url = https://example.awsapps.com/start
sso_region = us-east-1
sso_account_id = 444455556666
sso_role_name = Admin

[profile vanilla]
region = us-east-2
"#;
        let profiles = parse(cfg);
        assert_eq!(profiles.len(), 3);
        assert_eq!(profiles[0].source, ProfileSource::Sso);
        assert_eq!(profiles[1].source, ProfileSource::Sso);
        assert_eq!(profiles[2].source, ProfileSource::Cli);
    }

    #[test]
    fn ignores_non_profile_sections() {
        let cfg = r#"
[sso-session main]
sso_start_url = https://example.awsapps.com/start

[services foo]
s3 = ...

[profile real]
region = us-east-1
"#;
        let profiles = parse(cfg);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "real");
    }

    #[test]
    fn ignores_comments() {
        let cfg = r#"
# top-level comment
; another comment
[default]
# region = wrong
region = us-east-1
"#;
        let profiles = parse(cfg);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "default");
    }

    #[test]
    fn rejects_invalid_profile_names() {
        assert!(!is_valid_profile_name(""));
        assert!(!is_valid_profile_name("with space"));
        assert!(!is_valid_profile_name("semi;colon"));
        assert!(!is_valid_profile_name("pipe|chain"));
        assert!(!is_valid_profile_name("$(whoami)"));
        assert!(!is_valid_profile_name("../etc/passwd"));
        assert!(!is_valid_profile_name(&"a".repeat(129)));
    }

    #[test]
    fn accepts_valid_profile_names() {
        assert!(is_valid_profile_name("default"));
        assert!(is_valid_profile_name("dev"));
        assert!(is_valid_profile_name("prod-1"));
        assert!(is_valid_profile_name("team_a"));
        assert!(is_valid_profile_name("acme.us-east"));
    }
}
