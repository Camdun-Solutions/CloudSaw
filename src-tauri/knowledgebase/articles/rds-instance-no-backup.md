# RDS instance has no automated backups

## Description
The RDS instance has its `BackupRetentionPeriod` set to 0, which disables automated backups entirely. Without automated backups there is no point-in-time recovery and no daily snapshot lineage.

## Risk
A corrupted database, accidental destructive query, or ransomware event is unrecoverable without backups. Manual snapshots are not a substitute — they age out of operator memory and lack PITR granularity.

## Detection Logic
The scanner calls `rds:DescribeDBInstances` and flags any instance whose `BackupRetentionPeriod` is 0.

## Remediation
Set the backup retention period to at least 7 days (most workloads want 14–35). Ensure the maintenance window does not collide with the backup window.

## Terraform Fix
```hcl
resource "aws_db_instance" "primary" {
  backup_retention_period = 14
  backup_window           = "03:00-04:00"
  copy_tags_to_snapshot   = true
  # ...
}
```

## AWS CLI Fix
```sh
aws rds modify-db-instance \
  --db-instance-identifier <id> \
  --backup-retention-period 14 \
  --apply-immediately
```

## False Positives
Genuinely-ephemeral read replicas can run with backups off because the primary holds the canonical state — but the primary should always have backups.
