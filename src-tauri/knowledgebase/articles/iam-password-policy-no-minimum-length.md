# IAM password policy lacks minimum length

## Description
The IAM account password policy does not enforce a minimum password length, or the configured length is shorter than the recommended 14 characters. Without a minimum length, users can choose trivially short passwords that are vulnerable to brute-force and credential-stuffing attacks.

## Risk
Short passwords drastically reduce the search space for offline cracking and online guessing. AWS console credentials are a high-value target — a compromised IAM user can read or modify any resource the user has access to, and is often the first step in a broader account takeover.

## Detection Logic
The scanner reads the account password policy via `iam:GetAccountPasswordPolicy` and flags any policy whose `MinimumPasswordLength` is unset or less than 14.

## Remediation
Update the account password policy to require a minimum length of at least 14 characters and enable the other complexity options (uppercase, lowercase, numbers, symbols). Combine this with mandatory MFA on every human user.

## Terraform Fix
```hcl
resource "aws_iam_account_password_policy" "strict" {
  minimum_password_length        = 14
  require_lowercase_characters   = true
  require_uppercase_characters   = true
  require_numbers                = true
  require_symbols                = true
  allow_users_to_change_password = true
  password_reuse_prevention      = 24
  max_password_age               = 90
  hard_expiry                    = false
}
```

## AWS CLI Fix
```sh
aws iam update-account-password-policy \
  --minimum-password-length 14 \
  --require-symbols \
  --require-numbers \
  --require-uppercase-characters \
  --require-lowercase-characters \
  --allow-users-to-change-password \
  --password-reuse-prevention 24 \
  --max-password-age 90
```

## False Positives
Accounts that use IAM Identity Center (SSO) exclusively and have no IAM users with console passwords are functionally unaffected by this policy, even though the scanner still reports it. Apply the policy anyway as a defence-in-depth measure: it costs nothing and protects against future drift.
