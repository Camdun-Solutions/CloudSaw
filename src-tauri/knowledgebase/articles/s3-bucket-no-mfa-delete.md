# S3 bucket does not require MFA-Delete

## Description
The S3 bucket does not enforce MFA-Delete on permanent version deletion or on suspension of versioning. Without MFA-Delete, a compromised user can wipe versioned history with no second-factor friction.

## Risk
Ransomware and insider-threat attacks commonly delete versioned backups before exfiltration. MFA-Delete blocks those operations unless a current MFA token is presented by the root user — the only AWS principal that can configure MFA-Delete.

## Detection Logic
The scanner calls `s3:GetBucketVersioning`; buckets where `MFADelete` is `Disabled` are flagged.

## Remediation
Enable MFA-Delete from the root user account using a hardware MFA device. The configuration cannot be set programmatically through STS/assume-role and is only available via the root credentials.

## Terraform Fix
Terraform cannot configure MFA-Delete because it requires the root user's credentials and live MFA token. Document the manual procedure in the bucket's runbook.

## AWS CLI Fix
```sh
# Sign in as the root user, then run with the root user's credentials AND a current MFA code:
aws s3api put-bucket-versioning --bucket <bucket> \
  --versioning-configuration Status=Enabled,MFADelete=Enabled \
  --mfa "<mfa-serial> <code>"
```

## False Positives
Buckets with mature, isolated backup pipelines (e.g. AWS Backup with separate retention controls) sometimes substitute Object Lock for MFA-Delete. Either control is acceptable; absence of both is the actionable signal.
