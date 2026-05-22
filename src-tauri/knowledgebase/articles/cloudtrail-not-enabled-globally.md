# CloudTrail is not enabled across all regions

## Description
The account does not have at least one multi-region CloudTrail enabled. Activity outside the enabled regions is not logged at all.

## Risk
Attackers commonly pivot into rarely-used regions specifically because organisations enable CloudTrail only in their primary region. Without a multi-region trail, lateral movement into an unmonitored region produces no audit record.

## Detection Logic
The scanner calls `cloudtrail:DescribeTrails` and flags an account that has no trail whose `IsMultiRegionTrail` is `true` AND whose `IsLogging` is `true`.

## Remediation
Create a single multi-region trail per account (or rely on the Organizations-level trail). Log events to a centralised account-isolated S3 bucket with Object Lock enabled to prevent tampering.

## Terraform Fix
```hcl
resource "aws_cloudtrail" "all_regions" {
  name                          = "all-regions"
  s3_bucket_name                = aws_s3_bucket.trail.id
  is_multi_region_trail         = true
  enable_log_file_validation    = true
  include_global_service_events = true
  kms_key_id                    = aws_kms_key.trail.arn

  event_selector {
    read_write_type           = "All"
    include_management_events = true
  }
}
```

## AWS CLI Fix
```sh
aws cloudtrail create-trail \
  --name all-regions \
  --s3-bucket-name <bucket> \
  --is-multi-region-trail \
  --enable-log-file-validation \
  --kms-key-id <kms-arn>
aws cloudtrail start-logging --name all-regions
```

## False Positives
Accounts managed by an AWS Organization with a centralised organization trail are covered transparently; the per-account scan still flags them and the right answer is to document the Organization trail as the controlling trail.
