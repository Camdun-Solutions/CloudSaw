# IAM user has console access without MFA

## Description
The IAM user has a console password but no multi-factor authentication device enrolled. The user is therefore protected by exactly one factor — the password — which can be stolen via phishing, malware, or reuse.

## Risk
Without MFA, any compromise of the user's password yields full console access. AWS account takeovers overwhelmingly target IAM users without MFA because they are the cheapest path in.

## Detection Logic
For each IAM user, the scanner checks whether `iam:ListMFADevices` returns at least one device. Users with a `LoginProfile` (i.e. console-enabled) and no MFA device are flagged.

## Remediation
Enrol the user in a virtual or hardware MFA device immediately. Prefer hardware (YubiKey, Titan) for any administrative or break-glass user; virtual MFA (Authy, 1Password, OS authenticator) is acceptable for less-privileged users. Migrating to IAM Identity Center with mandatory MFA replaces the underlying problem entirely.

## Terraform Fix
```hcl
# MFA enrolment is interactive and must be performed by the user themselves
# in the AWS Console. Terraform can enforce that all human access goes
# through Identity Center, eliminating this class of IAM-user problem:
data "aws_caller_identity" "current" {}

# Optionally, deny actions when MFA is not present:
resource "aws_iam_policy" "require_mfa" {
  name = "RequireMfaForConsoleUsers"
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect   = "Deny"
      Action   = "*"
      Resource = "*"
      Condition = {
        BoolIfExists = { "aws:MultiFactorAuthPresent" = "false" }
      }
    }]
  })
}
```

## AWS CLI Fix
```sh
# Verify the user has no MFA device:
aws iam list-mfa-devices --user-name <user>
# Enrolment of a virtual MFA device:
aws iam create-virtual-mfa-device --virtual-mfa-device-name <user>
aws iam enable-mfa-device --user-name <user> \
  --serial-number arn:aws:iam::<account>:mfa/<user> \
  --authentication-code-1 <code1> --authentication-code-2 <code2>
```

## False Positives
A user with no `LoginProfile` (programmatic access only) is not flagged by this rule because MFA on access keys is enforced separately. Service users created for CI should not have console access at all.
