// Add a new AWS CLI profile (PR #66).
//
// CloudSaw historically REFUSED to touch `~/.aws/credentials` (see
// the comment block in `profiles.rs`): the SDK provider chain reads
// it implicitly, but the app never opened it. PR #66 introduces a
// narrow, user-initiated WRITE path so the user can populate the
// file from inside CloudSaw without dropping to a terminal and
// running `aws configure`.
//
// What this file does:
//   - Validates the profile name with the same character set used
//     elsewhere in the module (`profiles::is_valid_profile_name`).
//   - Appends a new section to `~/.aws/credentials` with the
//     `aws_access_key_id` + `aws_secret_access_key` keys.
//   - Appends a corresponding section to `~/.aws/config` with the
//     `region` + `output` keys (when supplied).
//   - Refuses to overwrite an existing profile section. Duplicates
//     are surfaced as `AuthError::DuplicateProfileName` so the UI
//     can localize the error and tell the user to pick a different
//     name.
//
// What this file deliberately does NOT do:
//   - Cache, log, or transmit the secret access key anywhere. The
//     secret arrives via IPC, is written to `~/.aws/credentials`,
//     and is dropped from the process before the function returns.
//   - Mutate existing sections — there is no "update" path here.
//     Users who want to replace a profile delete the section from
//     `~/.aws/credentials` themselves and re-add via the modal.
//   - Read existing `~/.aws/credentials` BODY. We only parse enough
//     to detect a section-header collision; key/value rows are
//     skipped untouched.
//
// File-permission hygiene:
//   - On Unix, `~/.aws/credentials` is chmod'd to 0600 after every
//     write. This matches the AWS CLI's own behavior on a fresh
//     `aws configure` invocation.
//   - On Windows, ACLs inherit from `%USERPROFILE%\.aws\`, which is
//     already user-only by virtue of being inside `%USERPROFILE%`.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use serde::Deserialize;
use zeroize::Zeroizing;

use super::error::AuthError;
use super::profiles::{config_path, is_valid_profile_name};

/// IPC input payload for `auth_create_profile`. The `secret_access_key`
/// arrives as a plain `String` because `Zeroizing<String>` does not
/// implement `serde::Deserialize`; `create_profile` wraps it in
/// `Zeroizing` immediately on receipt so the post-IPC lifetime of
/// the secret string has its backing memory wiped on drop.
#[derive(Deserialize)]
pub struct AddAwsProfileInput {
    pub name: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub output_format: Option<String>,
}

/// Internal mirror of `AddAwsProfileInput` with the secret wrapped in
/// `Zeroizing`. The function-local lifetime drops this struct on
/// return, which wipes the secret-bearing memory.
struct AddAwsProfileInputInternal {
    name: String,
    access_key_id: String,
    secret_access_key: Zeroizing<String>,
    region: Option<String>,
    output_format: Option<String>,
}

/// Returns the path the AWS SDK would resolve as the credentials file
/// (mirrors `profiles::config_path`'s logic but for credentials).
fn credentials_path() -> Option<PathBuf> {
    if let Ok(override_path) = std::env::var("AWS_SHARED_CREDENTIALS_FILE") {
        if !override_path.is_empty() {
            return Some(PathBuf::from(override_path));
        }
    }
    dirs::home_dir().map(|h| h.join(".aws").join("credentials"))
}

/// Adds a new AWS CLI profile to `~/.aws/credentials` (key material)
/// and `~/.aws/config` (region + output). Returns the profile name
/// on success.
pub fn create_profile(input: AddAwsProfileInput) -> Result<String, AuthError> {
    // Wrap the secret in `Zeroizing` right away so the rest of the
    // function (including the file-write path) holds the secret in
    // memory that's wiped when this scope returns.
    let secret = Zeroizing::new(input.secret_access_key);
    let input = AddAwsProfileInputInternal {
        name: input.name,
        access_key_id: input.access_key_id,
        secret_access_key: secret,
        region: input.region,
        output_format: input.output_format,
    };

    // 1. Validate input.
    if !is_valid_profile_name(&input.name) {
        return Err(AuthError::InvalidProfileName);
    }
    if input.access_key_id.trim().is_empty() || input.secret_access_key.is_empty() {
        return Err(AuthError::Internal("missing_credentials"));
    }

    // 2. Resolve paths. Bail with ConfigUnreadable if no home dir is
    //    resolvable — this matches the rest of the module's behavior
    //    and prevents writing to an inferred location the user can't
    //    audit.
    let creds_path = credentials_path().ok_or(AuthError::ConfigUnreadable)?;
    let config_path_buf = config_path().ok_or(AuthError::ConfigUnreadable)?;
    let aws_dir = creds_path
        .parent()
        .ok_or(AuthError::Internal("creds_path_no_parent"))?
        .to_path_buf();

    // 3. Ensure ~/.aws exists.
    if !aws_dir.exists() {
        fs::create_dir_all(&aws_dir).map_err(|_| AuthError::Internal("mkdir_aws_dir"))?;
    }

    // 4. Refuse to overwrite an existing profile in EITHER file. The
    //    AWS CLI would silently update — we don't, because the user
    //    can't undo a key-material overwrite. The frontend already
    //    checks the loaded profile list before submit; this is the
    //    server-side defense.
    let creds_content = read_or_empty(&creds_path)?;
    let config_content = read_or_empty(&config_path_buf)?;
    if has_section(&creds_content, &creds_section_header(&input.name))
        || has_section(&config_content, &config_section_header(&input.name))
    {
        return Err(AuthError::DuplicateProfileName);
    }

    // 5. Append the new section to credentials. Use OpenOptions with
    //    append=true so we don't truncate existing content.
    {
        let mut f = open_for_append(&creds_path)?;
        let block = render_credentials_block(&input);
        f.write_all(block.as_bytes())
            .map_err(|_| AuthError::ConfigWriteFailed("credentials_write"))?;
    }

    // 6. Same for config — only when at least one of region/output
    //    was supplied. The SDK / CLI happily run without a config
    //    section, so a no-op write here is harmless but noisy.
    if input.region.is_some() || input.output_format.is_some() {
        let mut f = open_for_append(&config_path_buf)?;
        let block = render_config_block(&input);
        f.write_all(block.as_bytes())
            .map_err(|_| AuthError::ConfigWriteFailed("config_write"))?;
    }

    // 7. Tighten permissions on credentials. AWS CLI sets 0600 on a
    //    fresh write, and we match that. No-op on Windows (NTFS ACLs
    //    inherit from the parent dir, which is inside %USERPROFILE%).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&creds_path, fs::Permissions::from_mode(0o600));
    }

    Ok(input.name)
}

fn read_or_empty(path: &std::path::Path) -> Result<String, AuthError> {
    match fs::read_to_string(path) {
        Ok(s) => Ok(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(_) => Err(AuthError::ConfigUnreadable),
    }
}

fn open_for_append(path: &std::path::Path) -> Result<File, AuthError> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|_| AuthError::ConfigWriteFailed("open"))
}

fn creds_section_header(name: &str) -> String {
    format!("[{name}]")
}

fn config_section_header(name: &str) -> String {
    // AWS quirk: `[default]` in ~/.aws/config has no "profile " prefix;
    // every other named profile is `[profile NAME]`.
    if name == "default" {
        "[default]".to_string()
    } else {
        format!("[profile {name}]")
    }
}

/// True iff `content` already contains a section header equal to
/// `header` on its own line. Comments and whitespace lines are
/// ignored. We intentionally do not parse key=value pairs — only
/// section headers matter for the duplicate check.
fn has_section(content: &str, header: &str) -> bool {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed == header {
            return true;
        }
    }
    false
}

fn render_credentials_block(input: &AddAwsProfileInputInternal) -> String {
    let mut s = String::new();
    s.push('\n');
    s.push_str(&creds_section_header(&input.name));
    s.push('\n');
    s.push_str("aws_access_key_id = ");
    s.push_str(input.access_key_id.trim());
    s.push('\n');
    s.push_str("aws_secret_access_key = ");
    s.push_str(input.secret_access_key.as_str());
    s.push('\n');
    s
}

fn render_config_block(input: &AddAwsProfileInputInternal) -> String {
    let mut s = String::new();
    s.push('\n');
    s.push_str(&config_section_header(&input.name));
    s.push('\n');
    if let Some(region) = input.region.as_ref() {
        let r = region.trim();
        if !r.is_empty() {
            s.push_str("region = ");
            s.push_str(r);
            s.push('\n');
        }
    }
    if let Some(output) = input.output_format.as_ref() {
        let o = output.trim();
        if !o.is_empty() {
            s.push_str("output = ");
            s.push_str(o);
            s.push('\n');
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_name() {
        let result = create_profile(AddAwsProfileInput {
            name: "name with spaces".into(),
            access_key_id: "AKIA".into(),
            secret_access_key: "secret".into(),
            region: None,
            output_format: None,
        });
        assert!(matches!(result, Err(AuthError::InvalidProfileName)));
    }

    #[test]
    fn rejects_empty_credentials() {
        let result = create_profile(AddAwsProfileInput {
            name: "ok-name".into(),
            access_key_id: "".into(),
            secret_access_key: "secret".into(),
            region: None,
            output_format: None,
        });
        assert!(matches!(result, Err(AuthError::Internal(_))));
    }

    #[test]
    fn renders_credentials_block_with_trim() {
        let input = AddAwsProfileInputInternal {
            name: "demo".into(),
            access_key_id: "  AKIAEXAMPLE  ".into(),
            secret_access_key: Zeroizing::new("secret-value".into()),
            region: None,
            output_format: None,
        };
        let block = render_credentials_block(&input);
        assert!(block.contains("[demo]"));
        assert!(block.contains("aws_access_key_id = AKIAEXAMPLE\n"));
        assert!(block.contains("aws_secret_access_key = secret-value\n"));
        assert!(!block.contains("  AKIAEXAMPLE"));
    }

    #[test]
    fn renders_config_block_with_default_marker() {
        let input = AddAwsProfileInputInternal {
            name: "default".into(),
            access_key_id: "AKIA".into(),
            secret_access_key: Zeroizing::new("s".into()),
            region: Some("us-west-2".into()),
            output_format: Some("json".into()),
        };
        let block = render_config_block(&input);
        assert!(block.contains("[default]"));
        assert!(!block.contains("[profile default]"));
        assert!(block.contains("region = us-west-2\n"));
        assert!(block.contains("output = json\n"));
    }

    #[test]
    fn renders_config_block_named_profile_uses_profile_prefix() {
        let input = AddAwsProfileInputInternal {
            name: "demo".into(),
            access_key_id: "AKIA".into(),
            secret_access_key: Zeroizing::new("s".into()),
            region: Some("us-east-1".into()),
            output_format: None,
        };
        let block = render_config_block(&input);
        assert!(block.contains("[profile demo]"));
        assert!(block.contains("region = us-east-1\n"));
        assert!(!block.contains("output = "));
    }

    #[test]
    fn has_section_finds_named_header() {
        let content =
            "\n# a comment\n[profile foo]\nregion = us-east-1\n\n[profile bar]\noutput=json\n";
        assert!(has_section(content, "[profile foo]"));
        assert!(has_section(content, "[profile bar]"));
        assert!(!has_section(content, "[profile baz]"));
        assert!(!has_section(content, "[foo]"));
    }
}
