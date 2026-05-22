// Input validation for the accounts module.
//
// Every public function in `accounts` validates its inputs *before* touching
// SQLite or the SDK, so a bad value from the frontend can never appear in a
// query, in a log line, or in an STS call. The validators here are intentionally
// strict — they reject anything outside a printable, well-defined character
// set so the values cannot smuggle shell metacharacters or control bytes.

use super::error::AccountsError;
use crate::auth::profiles::is_valid_profile_name;

/// Max user-supplied label length. Generous enough for "dev / acme-prod /
/// foobar staging", short enough to keep the table renderable on small
/// windows.
const MAX_LABEL_LEN: usize = 64;
const MIN_LABEL_LEN: usize = 1;

/// The four environment tags Contract 04 enumerates. Stored as TEXT so adding
/// a fifth later is a no-migration change.
const ALLOWED_ENVIRONMENTS: &[&str] = &["dev", "staging", "prod", "other"];

pub fn validate_label(label: &str) -> Result<(), AccountsError> {
    let trimmed = label.trim();
    if trimmed.len() < MIN_LABEL_LEN || trimmed.len() != label.len() {
        // Reject leading/trailing whitespace AND too-short labels with one
        // code — both look like the same kind of "fix your input" issue.
        return Err(AccountsError::InvalidInput("label"));
    }
    if label.chars().count() > MAX_LABEL_LEN {
        return Err(AccountsError::InvalidInput("label"));
    }
    if label
        .chars()
        .any(|c| c.is_control() || c == '\n' || c == '\r' || c == '\t')
    {
        return Err(AccountsError::InvalidInput("label"));
    }
    Ok(())
}

pub fn validate_profile_name(profile: &str) -> Result<(), AccountsError> {
    if !is_valid_profile_name(profile) {
        return Err(AccountsError::InvalidInput("profile_name"));
    }
    Ok(())
}

pub fn validate_environment(env: &str) -> Result<(), AccountsError> {
    if ALLOWED_ENVIRONMENTS.contains(&env) {
        Ok(())
    } else {
        Err(AccountsError::InvalidInput("environment"))
    }
}

/// AWS account IDs are exactly 12 ASCII digits. We never accept a UI-supplied
/// value for this — it is always the result of `sts:GetCallerIdentity` — but
/// the check exists so a defective SDK response cannot land a malformed row.
pub fn validate_aws_account_id(id: &str) -> Result<(), AccountsError> {
    if id.len() != 12 || !id.chars().all(|c| c.is_ascii_digit()) {
        return Err(AccountsError::Internal("malformed_aws_account_id"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_rejects_empty_and_whitespace_padded() {
        assert!(validate_label("").is_err());
        assert!(validate_label("   ").is_err());
        assert!(validate_label(" dev").is_err());
        assert!(validate_label("dev ").is_err());
        assert!(validate_label("dev").is_ok());
    }

    #[test]
    fn label_rejects_control_chars_and_overlong() {
        assert!(validate_label("dev\nprod").is_err());
        assert!(validate_label("dev\tprod").is_err());
        assert!(validate_label("dev\0prod").is_err());
        assert!(validate_label(&"x".repeat(MAX_LABEL_LEN + 1)).is_err());
        assert!(validate_label(&"x".repeat(MAX_LABEL_LEN)).is_ok());
    }

    #[test]
    fn environment_only_accepts_enumerated_values() {
        for ok in &["dev", "staging", "prod", "other"] {
            assert!(validate_environment(ok).is_ok());
        }
        for bad in &["", "DEV", "production", "test", "  dev"] {
            assert!(
                validate_environment(bad).is_err(),
                "expected reject for {bad:?}"
            );
        }
    }

    #[test]
    fn aws_account_id_must_be_12_digits() {
        assert!(validate_aws_account_id("111122223333").is_ok());
        assert!(validate_aws_account_id("1234").is_err());
        assert!(validate_aws_account_id("11112222333a").is_err());
        assert!(validate_aws_account_id(" 11122223333").is_err());
        assert!(validate_aws_account_id("1111222233334").is_err());
    }
}
