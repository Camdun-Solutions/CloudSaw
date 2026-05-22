# IAM password policy does not require uppercase characters

## Description
The IAM account password policy does not require at least one uppercase character. Combined with other complexity gaps this materially weakens passwords against dictionary and rule-based cracking attacks.

## Risk
Passwords that lack character-class diversity are easier to crack offline and easier to guess online. Enforcing all four complexity classes (uppercase, lowercase, number, symbol) and a long minimum length together is the minimum bar for human-chosen passwords.

## Detection Logic
The scanner reads the account password policy via `iam:GetAccountPasswordPolicy` and flags any policy whose `RequireUppercaseCharacters` is `false` or absent.

## Remediation
Enable the uppercase-character requirement on the account password policy. The full strict policy below covers all four character-class requirements at once.

## Terraform Fix
```hcl
resource "aws_iam_account_password_policy" "strict" {
  require_uppercase_characters = true
  # ...other complexity options...
}
```

## AWS CLI Fix
```sh
aws iam update-account-password-policy --require-uppercase-characters
```

## False Positives
None. The cost of this control is zero and the protection is universal.
