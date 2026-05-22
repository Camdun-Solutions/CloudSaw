# IAM password policy does not prevent password reuse

## Description
The IAM account password policy does not prevent users from reusing their previous passwords. Combined with mandatory rotation, this is meant to ensure that a "rotation" actually replaces the credential rather than recycling it.

## Risk
Reused passwords defeat rotation entirely. A breached password remains valid every time the user rotates back to it.

## Detection Logic
The scanner reads the account password policy via `iam:GetAccountPasswordPolicy` and flags any policy whose `PasswordReusePrevention` is unset or less than 24.

## Remediation
Set the password reuse prevention count to at least 24 (the AWS-recommended value).

## Terraform Fix
```hcl
resource "aws_iam_account_password_policy" "strict" {
  password_reuse_prevention = 24
}
```

## AWS CLI Fix
```sh
aws iam update-account-password-policy --password-reuse-prevention 24
```

## False Positives
None.
