# IAM password policy does not require symbols

## Description
The IAM account password policy does not require at least one symbol character (`!`, `@`, `#`, etc.). This is one of the four standard character-class complexity controls.

## Risk
Passwords without symbol characters fall to cracking dictionaries and rule-based attacks more readily than those that use the full printable-ASCII space.

## Detection Logic
The scanner reads the account password policy via `iam:GetAccountPasswordPolicy` and flags any policy whose `RequireSymbols` is `false` or absent.

## Remediation
Enable the symbol-character requirement on the account password policy.

## Terraform Fix
```hcl
resource "aws_iam_account_password_policy" "strict" {
  require_symbols = true
}
```

## AWS CLI Fix
```sh
aws iam update-account-password-policy --require-symbols
```

## False Positives
Some passphrase-style password managers default to alphanumeric output for compatibility reasons. If a deployed password manager cannot emit symbols, raise the minimum length to compensate.
