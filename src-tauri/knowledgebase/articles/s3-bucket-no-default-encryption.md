# S3 bucket has no default encryption

## Description
The S3 bucket does not have a default server-side encryption configuration. New objects uploaded without an explicit encryption header will not be encrypted at rest unless the caller specifies it on every put.

## Risk
Unencrypted objects rely on bucket-policy enforcement and caller discipline alone. A single misconfigured uploader will silently leave plaintext on the bucket — a finding that often only surfaces during an audit.

## Detection Logic
The scanner calls `s3:GetBucketEncryption`; buckets where the call returns `ServerSideEncryptionConfigurationNotFoundError` are flagged.

## Remediation
Enable default encryption on every bucket. SSE-S3 (AES-256) is the no-cost minimum; SSE-KMS with a customer-managed key adds audit-trail and granular access control at the cost of per-call KMS charges.

## Terraform Fix
```hcl
resource "aws_s3_bucket_server_side_encryption_configuration" "default" {
  bucket = aws_s3_bucket.example.id
  rule {
    apply_server_side_encryption_by_default {
      sse_algorithm     = "aws:kms"
      kms_master_key_id = aws_kms_key.bucket.arn
    }
    bucket_key_enabled = true
  }
}
```

## AWS CLI Fix
```sh
aws s3api put-bucket-encryption --bucket <bucket> \
  --server-side-encryption-configuration '{
    "Rules": [{
      "ApplyServerSideEncryptionByDefault": {"SSEAlgorithm": "AES256"},
      "BucketKeyEnabled": true
    }]
  }'
```

## False Positives
AWS now enables SSE-S3 by default on new buckets. Buckets created before that change still need a one-time explicit configuration. There is no legitimate reason to opt out.
