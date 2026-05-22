# KMS customer-managed key has rotation disabled

## Description
The customer-managed KMS key has automatic annual key rotation disabled. Long-lived symmetric keys accumulate cryptographic exposure across the volume of data they have ever encrypted.

## Risk
A long-lived key with broad usage means a hypothetical compromise has a longer historical retro-impact. AWS automatic rotation preserves all prior key material in-place so existing ciphertexts decrypt without re-encryption, but new encryptions use fresh material.

## Detection Logic
The scanner calls `kms:GetKeyRotationStatus` on every customer-managed CMK and flags any key whose `KeyRotationEnabled` is `false`.

## Remediation
Enable annual rotation on every customer-managed CMK. AWS-managed keys (the `aws/*` aliases) rotate automatically and cannot be controlled by the customer.

## Terraform Fix
```hcl
resource "aws_kms_key" "example" {
  description         = "..."
  enable_key_rotation = true
}
```

## AWS CLI Fix
```sh
aws kms enable-key-rotation --key-id <key-id-or-arn>
```

## False Positives
External-key-store (XKS) keys and asymmetric keys cannot be rotated by AWS; the rule generally treats those as non-applicable.
