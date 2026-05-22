# IAM password policy does not require lowercase characters

## Description
The IAM account password policy does not require at least one lowercase character. Like the uppercase requirement, this is one of the four standard character-class controls.

## Risk
Passwords that lack character-class diversity are easier to crack and easier to guess. Enable all four classes together to materially raise the bar.

## Detection Logic
The scanner reads the account password policy via `iam:GetAccountPasswordPolicy` and flags any policy whose `RequireLowercaseCharacters` is `false` or absent.

## Remediation
Enable the lowercase-character requirement on the account password policy.

## Terraform Fix
```hcl
resource "aws_iam_account_password_policy" "strict" {
  require_lowercase_characters = true
}
```

## AWS CLI Fix
```sh
aws iam update-account-password-policy --require-lowercase-characters
```

## False Positives
None.
