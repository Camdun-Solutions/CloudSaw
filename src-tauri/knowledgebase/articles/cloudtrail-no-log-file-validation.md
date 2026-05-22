# CloudTrail log file validation is disabled

## Description
The CloudTrail trail does not have log-file validation enabled. Without validation, there is no cryptographic chain proving log files have not been tampered with after delivery to S3.

## Risk
An attacker that compromises the log bucket can quietly delete or alter CloudTrail records. Log-file validation publishes a digest file per hour that lets you detect any post-delivery modification.

## Detection Logic
The scanner calls `cloudtrail:DescribeTrails` and flags any trail whose `LogFileValidationEnabled` is `false`.

## Remediation
Enable log-file validation on every trail. The validation itself runs server-side; clients can verify integrity later using `aws cloudtrail validate-logs`.

## Terraform Fix
```hcl
resource "aws_cloudtrail" "primary" {
  enable_log_file_validation = true
}
```

## AWS CLI Fix
```sh
aws cloudtrail update-trail --name <name> --enable-log-file-validation
```

## False Positives
None.
