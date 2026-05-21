// Caller-identity resolution for the IAM role's trust policy.
//
// Contract 05 §Constraints:
//   * "The IAM role's trust policy principal MUST be set to the **current
//     caller's ARN**, fetched live via `sts:GetCallerIdentity` at provisioning
//     time. It MUST NOT be a user-supplied value and MUST NOT be a wildcard."
//   * "If the caller identity is a federated/assumed-role session, the trust
//     policy MUST resolve to the underlying role ARN, not the transient
//     session ARN."
//
// The unwrapping rules baked in here:
//
//   1. `arn:aws:iam::ACCT:user/...`          → use as-is.
//   2. `arn:aws:iam::ACCT:root`              → use as-is (account root).
//   3. `arn:aws:iam::ACCT:role/...`          → use as-is.
//   4. `arn:aws:sts::ACCT:assumed-role/NAME/SESSION` →
//        a. NAME starts with `AWSReservedSSO_` → produce
//             `arn:aws:iam::ACCT:role/aws-reserved/sso.amazonaws.com/NAME`
//             (IAM Identity Center reserved-role path).
//        b. otherwise → `arn:aws:iam::ACCT:role/NAME`.
//   5. Anything else                         → IdentityUnresolvable.
//
// Refusing to fall back to a wildcard is deliberate. The variable validator
// in `tf-modules/scanner-role/variables.tf` would block a wildcard at the
// Terraform layer anyway, but raising here gives a stable error code the UI
// can localize cleanly (Contract 05 §Edge Cases: federated-SSO row).

use super::error::TerraformError;

/// Reduce a `sts:GetCallerIdentity` ARN to the principal CloudSaw will write
/// into the trust policy. Returns the trust-principal ARN on success.
pub fn underlying_principal_arn(arn: &str) -> Result<String, TerraformError> {
    // Cheap structural checks first. We never want to invoke any kind of
    // network parsing or regex backtracking on a frontend-controlled value;
    // here the value comes from `sts:GetCallerIdentity` so we trust it more,
    // but defensive parsing keeps the code stable across SDK quirks.
    let parts: Vec<&str> = arn.split(':').collect();
    if parts.len() != 6 || parts[0] != "arn" {
        return Err(TerraformError::IdentityUnresolvable);
    }
    let partition = parts[1]; // "aws", "aws-us-gov", "aws-cn"
    let service = parts[2]; // "iam" or "sts"
    let account = parts[4];
    let resource = parts[5];

    if !valid_account_id(account) {
        return Err(TerraformError::IdentityUnresolvable);
    }
    if !valid_partition(partition) {
        return Err(TerraformError::IdentityUnresolvable);
    }

    match service {
        "iam" => {
            // IAM ARNs are already the underlying principal form. We require
            // a non-empty resource name after the prefix so a bare `role/` or
            // `user/` is rejected.
            let ok = matches!(resource, "root")
                || resource
                    .strip_prefix("user/")
                    .map(|n| !n.is_empty())
                    .unwrap_or(false)
                || resource
                    .strip_prefix("role/")
                    .map(|n| !n.is_empty())
                    .unwrap_or(false);
            if ok {
                Ok(arn.to_string())
            } else {
                Err(TerraformError::IdentityUnresolvable)
            }
        }
        "sts" => {
            // The only sts form we accept is assumed-role. STS federated-user
            // ARNs (`assumed-role/.../...` from sts:AssumeRoleWithSAML or
            // sts:AssumeRoleWithWebIdentity) still match this shape.
            if let Some(rest) = resource.strip_prefix("assumed-role/") {
                let (role_name, _session) = match rest.split_once('/') {
                    Some(pair) => pair,
                    None => return Err(TerraformError::IdentityUnresolvable),
                };
                if role_name.is_empty() {
                    return Err(TerraformError::IdentityUnresolvable);
                }
                // SSO Identity Center reserved roles live under a fixed path.
                // The IAM ARN form must include that path; the simple form
                // `role/NAME` would not actually map to the same IAM role.
                let path = if role_name.starts_with("AWSReservedSSO_") {
                    "role/aws-reserved/sso.amazonaws.com/"
                } else {
                    "role/"
                };
                Ok(format!("arn:{partition}:iam::{account}:{path}{role_name}"))
            } else {
                Err(TerraformError::IdentityUnresolvable)
            }
        }
        _ => Err(TerraformError::IdentityUnresolvable),
    }
}

fn valid_account_id(s: &str) -> bool {
    s.len() == 12 && s.chars().all(|c| c.is_ascii_digit())
}

fn valid_partition(p: &str) -> bool {
    matches!(p, "aws" | "aws-us-gov" | "aws-cn")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iam_user_arn_passes_through() {
        let arn = "arn:aws:iam::111122223333:user/alice";
        assert_eq!(underlying_principal_arn(arn).unwrap(), arn);
    }

    #[test]
    fn iam_role_arn_passes_through() {
        let arn = "arn:aws:iam::111122223333:role/CICDDeployer";
        assert_eq!(underlying_principal_arn(arn).unwrap(), arn);
    }

    #[test]
    fn iam_root_passes_through() {
        let arn = "arn:aws:iam::111122223333:root";
        assert_eq!(underlying_principal_arn(arn).unwrap(), arn);
    }

    #[test]
    fn assumed_role_unwraps_to_iam_role_arn() {
        let session = "arn:aws:sts::111122223333:assumed-role/CICDDeployer/jenkins-build-1234";
        assert_eq!(
            underlying_principal_arn(session).unwrap(),
            "arn:aws:iam::111122223333:role/CICDDeployer"
        );
    }

    #[test]
    fn sso_assumed_role_uses_aws_reserved_path() {
        let session = "arn:aws:sts::111122223333:assumed-role/AWSReservedSSO_AdminAccess_a1b2c3/alice@example.com";
        assert_eq!(
            underlying_principal_arn(session).unwrap(),
            "arn:aws:iam::111122223333:role/aws-reserved/sso.amazonaws.com/AWSReservedSSO_AdminAccess_a1b2c3"
        );
    }

    #[test]
    fn govcloud_partition_is_preserved() {
        let arn = "arn:aws-us-gov:iam::123456789012:role/Auditor";
        assert_eq!(underlying_principal_arn(arn).unwrap(), arn);
    }

    #[test]
    fn govcloud_assumed_role_unwraps_correctly() {
        let session =
            "arn:aws-us-gov:sts::123456789012:assumed-role/Auditor/session";
        assert_eq!(
            underlying_principal_arn(session).unwrap(),
            "arn:aws-us-gov:iam::123456789012:role/Auditor"
        );
    }

    #[test]
    fn rejects_wildcard_and_malformed_arns() {
        for bad in [
            "*",
            "",
            "arn:aws:iam::111122223333:*",
            "arn:aws:iam::111122223333:group/admins",
            "arn:aws:sts::111122223333:federated-user/anonymous",
            "arn:aws:sts::111122223333:assumed-role/",
            "arn:aws:sts::111122223333:assumed-role/NoSession",
            "arn:aws:iam::not-an-id:role/x",
            "arn:aws:iam::111122223333:role/", // empty name
            "not-an-arn",
        ] {
            assert!(
                matches!(
                    underlying_principal_arn(bad),
                    Err(TerraformError::IdentityUnresolvable)
                ),
                "expected IdentityUnresolvable for {bad:?}"
            );
        }
    }

    #[test]
    fn rejects_unknown_partition() {
        let arn = "arn:aws-bogus:iam::111122223333:role/x";
        assert!(matches!(
            underlying_principal_arn(arn),
            Err(TerraformError::IdentityUnresolvable)
        ));
    }
}
