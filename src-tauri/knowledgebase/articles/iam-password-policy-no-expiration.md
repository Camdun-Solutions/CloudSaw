# IAM password policy does not enforce expiration

## Description
The IAM account password policy does not require passwords to expire and be rotated within a bounded interval (e.g. 90 days). Long-lived credentials accumulate exposure as devices, backups, and notes age.

## Risk
A stolen or leaked password remains valid indefinitely until detected. Combined with no MFA, this is a recipe for silent persistent access.

## Detection Logic
The scanner reads the account password policy via `iam:GetAccountPasswordPolicy` and flags any policy whose `MaxPasswordAge` is unset.

## Remediation
Set a maximum password age between 60 and 90 days for human users. Pair this with mandatory MFA so a rotation gap is not the only line of defence.

## Terraform Fix
```hcl
resource "aws_iam_account_password_policy" "strict" {
  max_password_age = 90
}
```

## AWS CLI Fix
```sh
aws iam update-account-password-policy --max-password-age 90
```

## False Positives
Recent NIST guidance discourages forced periodic rotation in favour of strong passwords plus breach-monitoring. If your account is fully covered by IAM Identity Center and breach-credential monitoring is in place, rotation cadence becomes less load-bearing — but the strict baseline still applies to any IAM users that exist.
