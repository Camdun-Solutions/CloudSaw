# RDS instance is publicly accessible

## Description
The RDS instance has `PubliclyAccessible = true`. Even with security-group restrictions, a publicly accessible database instance has its DNS name resolving to a public IP, expanding the attack surface.

## Risk
Public databases are routinely discovered through DNS enumeration and shodan-style scanning. Misconfigured security groups, default credentials, or unpatched CVEs become public-internet vulnerabilities the moment the DB endpoint resolves publicly.

## Detection Logic
The scanner calls `rds:DescribeDBInstances` and flags any instance with `PubliclyAccessible = true`.

## Remediation
Set the instance to private. If clients outside the VPC need access, route through a VPN, Transit Gateway, or PrivateLink endpoint rather than the public internet.

## Terraform Fix
```hcl
resource "aws_db_instance" "primary" {
  publicly_accessible = false
  # ...
}
```

## AWS CLI Fix
```sh
aws rds modify-db-instance \
  --db-instance-identifier <id> \
  --no-publicly-accessible \
  --apply-immediately
```

## False Positives
Rare. The cleanest "public DB" pattern is a separate read replica fronted by an API layer; even then, exposing the replica directly is usually the wrong architecture.
