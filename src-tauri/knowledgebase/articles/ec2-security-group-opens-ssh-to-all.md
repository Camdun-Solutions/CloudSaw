# Security group opens SSH (port 22) to the world

## Description
The security group has an ingress rule allowing TCP/22 from `0.0.0.0/0` or `::/0`. SSH is the single most-targeted port on the public internet — automated scanners attempt brute-force and CVE exploitation against any open SSH endpoint within minutes.

## Risk
Open SSH endpoints attract continuous brute-force attempts and become the first foothold in many breaches. Even with key-only auth, the exposure leaks fingerprintable information about the host and platform.

## Detection Logic
The scanner flags any security group ingress rule that combines port 22 with a `0.0.0.0/0` or `::/0` source CIDR.

## Remediation
Restrict SSH to a known bastion or VPN CIDR, or eliminate SSH entirely by adopting AWS Systems Manager Session Manager. Session Manager removes the need for inbound network rules altogether and provides IAM-gated, audit-logged shell access.

## Terraform Fix
```hcl
# Preferred: no inbound SSH at all; use Session Manager.
# If SSH is required, restrict the source to your bastion / VPN:
resource "aws_security_group_rule" "ssh_from_bastion" {
  security_group_id        = aws_security_group.app.id
  type                     = "ingress"
  from_port                = 22
  to_port                  = 22
  protocol                 = "tcp"
  source_security_group_id = aws_security_group.bastion.id
}
```

## AWS CLI Fix
```sh
aws ec2 revoke-security-group-ingress \
  --group-id <sg-id> --protocol tcp --port 22 --cidr 0.0.0.0/0
```

## False Positives
A bastion host that intentionally exposes SSH on 22 to the world (with key-only auth and fail2ban) is still flagged. Replace the host with Session Manager if at all possible — exposing SSH on the internet is hard to operate safely.
