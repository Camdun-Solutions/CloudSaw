# S3 bucket versioning is disabled

## Description
The S3 bucket does not have versioning enabled. Object overwrites and deletes immediately discard prior content, leaving no built-in recovery path.

## Risk
Accidental overwrites, mass-deletions from misbehaving code, and ransomware-style attacks all become unrecoverable without versioning. Versioning is also a prerequisite for cross-region replication and many lifecycle-based retention strategies.

## Detection Logic
The scanner calls `s3:GetBucketVersioning`; buckets whose `Status` is `null` or `Suspended` are flagged.

## Remediation
Enable versioning on every bucket that holds non-ephemeral data. Pair versioning with a lifecycle policy that transitions old versions to cheaper storage classes and expires them after a defined retention period.

## Terraform Fix
```hcl
resource "aws_s3_bucket_versioning" "example" {
  bucket = aws_s3_bucket.example.id
  versioning_configuration {
    status = "Enabled"
  }
}
```

## AWS CLI Fix
```sh
aws s3api put-bucket-versioning --bucket <bucket> \
  --versioning-configuration Status=Enabled
```

## False Positives
Strictly ephemeral buckets (e.g. build-artifact caches that rebuild from source on demand) can leave versioning off intentionally. Document the exception so the next audit doesn't relitigate it.
