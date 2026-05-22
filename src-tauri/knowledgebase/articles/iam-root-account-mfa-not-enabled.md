# Root account does not have MFA enabled

## Description
The AWS account root user does not have any MFA device enrolled. The root user has unrestricted access to the entire account and cannot be limited by IAM policies, so it is the single most valuable target in the account.

## Risk
Compromise of the root user is account-ending. The attacker can change billing details, delete every resource, lock you out of the account, and abuse it for crypto-mining at your expense. AWS will not undo a deliberate destructive action even with provable account ownership.

## Detection Logic
The scanner reads `iam:GetAccountSummary` and flags an account whose `AccountMFAEnabled` value is `0`.

## Remediation
1. Sign in as the root user.
2. Enrol a hardware MFA device (FIDO2 / YubiKey) — prefer hardware over virtual for root.
3. Print and physically secure the recovery codes.
4. Sign out and confirm subsequent root sign-ins are gated by the new MFA device.
5. Stop using the root user for daily operations — create an IAM Identity Center admin user for that.

## Terraform Fix
Root MFA enrolment is interactive and cannot be performed by Terraform. The account password policy and the deny-root-without-mfa SCP can be codified:

```hcl
# Deny everything from the root user when MFA is not present (in Organizations):
data "aws_iam_policy_document" "deny_root_no_mfa" {
  statement {
    effect    = "Deny"
    actions   = ["*"]
    resources = ["*"]
    condition {
      test     = "StringLike"
      variable = "aws:PrincipalArn"
      values   = ["arn:aws:iam::*:root"]
    }
    condition {
      test     = "BoolIfExists"
      variable = "aws:MultiFactorAuthPresent"
      values   = ["false"]
    }
  }
}
```

## AWS CLI Fix
Root MFA enrolment must be performed in the Console while signed in as the root user. The CLI cannot enrol a device for the root account.

## False Positives
None. Every account, every environment, every cost-center must have root MFA.
