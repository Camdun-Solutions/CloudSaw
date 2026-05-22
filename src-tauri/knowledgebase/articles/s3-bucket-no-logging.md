# S3 bucket access logging is disabled

## Description
The S3 bucket does not have access logging configured to deliver server-access logs to another bucket. Without these logs, after-the-fact attribution of who-read-what is impossible.

## Risk
During an incident, the inability to enumerate who accessed which objects in the affected bucket prevents both scoping and notification. Many data-breach disclosure regimes require this exact information.

## Detection Logic
The scanner calls `s3:GetBucketLogging`; buckets where `LoggingEnabled` is absent are flagged.

## Remediation
Configure access logging to a dedicated log-collection bucket in the same Region. The log bucket must itself be private, encrypted, and have its own access logging disabled (to avoid recursion).

## Terraform Fix
```hcl
resource "aws_s3_bucket_logging" "example" {
  bucket        = aws_s3_bucket.example.id
  target_bucket = aws_s3_bucket.logs.id
  target_prefix = "s3-access-logs/${aws_s3_bucket.example.id}/"
}
```

## AWS CLI Fix
```sh
aws s3api put-bucket-logging --bucket <bucket> \
  --bucket-logging-status '{
    "LoggingEnabled": {
      "TargetBucket": "<log-bucket>",
      "TargetPrefix": "s3-access-logs/<bucket>/"
    }
  }'
```

## False Positives
Buckets used purely as CloudFront origins typically rely on CloudFront access logs instead; for those buckets, S3-level access logging is largely redundant but still recommended.
