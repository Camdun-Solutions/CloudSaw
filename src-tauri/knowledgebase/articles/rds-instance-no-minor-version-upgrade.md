# RDS instance does not auto-apply minor version upgrades

## Description
The RDS instance has `AutoMinorVersionUpgrade = false`. Security patches that ship as minor-version updates accumulate uninstalled.

## Risk
Most RDS engine CVEs are patched via minor-version upgrades. Disabling auto-upgrade means each CVE accumulates manual operational overhead and creates a window of exposure between patch availability and deployment.

## Detection Logic
The scanner calls `rds:DescribeDBInstances` and flags any instance with `AutoMinorVersionUpgrade = false`.

## Remediation
Re-enable auto minor version upgrade. Choose a maintenance window when downtime is acceptable; for multi-AZ instances, the failover is brief but real.

## Terraform Fix
```hcl
resource "aws_db_instance" "primary" {
  auto_minor_version_upgrade = true
  maintenance_window         = "sun:05:00-sun:06:00"
}
```

## AWS CLI Fix
```sh
aws rds modify-db-instance \
  --db-instance-identifier <id> \
  --auto-minor-version-upgrade \
  --apply-immediately
```

## False Positives
Highly regulated workloads sometimes disable auto-upgrade and use scheduled patch windows under change-control. In those cases the control is satisfied by a documented patch cadence; otherwise re-enable auto-upgrade.
