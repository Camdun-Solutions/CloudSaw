# IAM password policy does not require numbers

## Description
The IAM account password policy does not require at least one numeric character. This is one of the four standard character-class complexity controls.

## Risk
Passwords without numeric characters are easier to brute-force, especially when combined with other complexity gaps. Cracking dictionaries strongly favour alphabetic-only inputs.

## Detection Logic
The scanner reads the account password policy via `iam:GetAccountPasswordPolicy` and flags any policy whose `RequireNumbers` is `false` or absent.

## Remediation
Enable the numeric-character requirement on the account password policy.

## Terraform Fix
```hcl
resource "aws_iam_account_password_policy" "strict" {
  require_numbers = true
}
```

## AWS CLI Fix
```sh
aws iam update-account-password-policy --require-numbers
```

## False Positives
None.
