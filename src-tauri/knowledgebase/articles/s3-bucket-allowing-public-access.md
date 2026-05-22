# S3 bucket allows public access

## Description
The S3 bucket grants read or write access to "Everyone" (`AllUsers`) or "Any Authenticated AWS User" (`AuthenticatedUsers`) through its ACL, bucket policy, or Public Access Block configuration. Either grants effectively expose bucket contents to the internet.

## Risk
Public S3 buckets are the highest-frequency cause of AWS data breaches in published incident reports. Once data lands in a public bucket, it must be assumed to be replicated by web crawlers and adversaries; recovery is impossible.

## Detection Logic
The scanner inspects each bucket's `s3:GetBucketAcl`, `s3:GetBucketPolicy`, `s3:GetBucketPolicyStatus`, and `s3:GetPublicAccessBlock`. A bucket is flagged when any grant resolves to a public principal AND the Public Access Block does not block it.

## Remediation
Apply the bucket-level and account-level Public Access Block to every account. Where genuine public hosting is required (e.g. static-site CDN origins), use CloudFront with an Origin Access Control instead of a directly-public bucket.

## Terraform Fix
```hcl
resource "aws_s3_bucket_public_access_block" "block" {
  bucket                  = aws_s3_bucket.example.id
  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}

# Account-level Public Access Block (apply once per account):
resource "aws_s3_account_public_access_block" "block" {
  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}
```

## AWS CLI Fix
```sh
aws s3api put-public-access-block --bucket <bucket> \
  --public-access-block-configuration \
  BlockPublicAcls=true,IgnorePublicAcls=true,BlockPublicPolicy=true,RestrictPublicBuckets=true

aws s3control put-public-access-block --account-id <account> \
  --public-access-block-configuration \
  BlockPublicAcls=true,IgnorePublicAcls=true,BlockPublicPolicy=true,RestrictPublicBuckets=true
```

## False Positives
Static-site buckets intentionally configured for public web hosting will be flagged. The right answer is still to front the bucket with CloudFront and OAC and lock the bucket itself down; if you must keep direct public access, document the exception and pin the article as reviewed.
