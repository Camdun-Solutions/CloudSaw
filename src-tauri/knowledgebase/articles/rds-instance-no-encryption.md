# RDS instance has no encryption at rest

## Description
The RDS instance was created without `StorageEncrypted = true`. The instance's data files, automated backups, snapshots, and read replicas all live on unencrypted storage.

## Risk
Unencrypted RDS storage fails most compliance baselines (HIPAA, PCI-DSS, FedRAMP). Even outside of compliance, an unencrypted snapshot accidentally shared cross-account becomes an immediate data-exposure risk.

## Detection Logic
The scanner calls `rds:DescribeDBInstances` and flags any instance with `StorageEncrypted = false`.

## Remediation
RDS does not support enabling encryption in place. Migrate by creating an encrypted snapshot and restoring to a new encrypted instance, then cut over the endpoint.

## Terraform Fix
```hcl
resource "aws_db_instance" "primary" {
  storage_encrypted = true
  kms_key_id        = aws_kms_key.rds.arn
  # ...
}
```

## AWS CLI Fix
```sh
# 1. Snapshot the unencrypted instance:
aws rds create-db-snapshot --db-instance-identifier <id> --db-snapshot-identifier <snap>
# 2. Copy the snapshot with encryption:
aws rds copy-db-snapshot --source-db-snapshot-identifier <snap> \
  --target-db-snapshot-identifier <snap-enc> --kms-key-id <kms-arn>
# 3. Restore the encrypted snapshot to a new instance and cut over.
```

## False Positives
None for steady-state. The encrypted-from-the-start default has been available for years.
