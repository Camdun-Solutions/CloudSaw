# CloudTrail logs are not KMS-encrypted

## Description
The CloudTrail trail does not encrypt log files with a customer-managed KMS key. Without KMS, log files are protected only by the bucket's default encryption — which provides no audit trail of who decrypted what.

## Risk
A compromised log-bucket access path immediately yields plaintext CloudTrail history. KMS encryption combined with a key policy that restricts decrypt-rights to a small set of principals (or only the SIEM ingester) creates a meaningful second barrier.

## Detection Logic
The scanner calls `cloudtrail:DescribeTrails` and flags any trail whose `KmsKeyId` is unset.

## Remediation
Configure a dedicated KMS key for CloudTrail with a key policy that allows only the trail-delivery service and a narrow list of decryptors. Update each trail to reference the key.

## Terraform Fix
```hcl
resource "aws_kms_key" "trail" {
  description             = "CloudTrail log encryption"
  enable_key_rotation     = true
  policy                  = data.aws_iam_policy_document.trail_key.json
}

resource "aws_cloudtrail" "primary" {
  kms_key_id = aws_kms_key.trail.arn
  # ...
}
```

## AWS CLI Fix
```sh
aws cloudtrail update-trail --name <name> --kms-key-id <kms-arn>
```

## False Positives
None.
