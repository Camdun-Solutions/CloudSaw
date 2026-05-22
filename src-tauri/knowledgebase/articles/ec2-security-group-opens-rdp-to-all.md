# Security group opens RDP (port 3389) to the world

## Description
The security group has an ingress rule allowing TCP/3389 from `0.0.0.0/0` or `::/0`. RDP is heavily targeted; many Windows-host compromises in AWS trace back to internet-exposed RDP.

## Risk
RDP brute-force and credential-stuffing attacks are continuous and automated. Exposed RDP is a leading vector for ransomware deployment on Windows hosts in cloud environments.

## Detection Logic
The scanner flags any security group ingress rule that combines port 3389 with a `0.0.0.0/0` or `::/0` source CIDR.

## Remediation
Replace direct RDP with AWS Systems Manager Fleet Manager (browser-based RDP via Session Manager), or restrict 3389 to a tightly scoped corporate-VPN CIDR.

## Terraform Fix
```hcl
# Replace with Session Manager / Fleet Manager; if direct RDP is required:
resource "aws_security_group_rule" "rdp_from_vpn" {
  security_group_id = aws_security_group.windows.id
  type              = "ingress"
  from_port         = 3389
  to_port           = 3389
  protocol          = "tcp"
  cidr_blocks       = ["10.42.0.0/16"]
}
```

## AWS CLI Fix
```sh
aws ec2 revoke-security-group-ingress \
  --group-id <sg-id> --protocol tcp --port 3389 --cidr 0.0.0.0/0
```

## False Positives
None for steady-state. Time-bounded exceptions during incident response should be revoked the moment the operation completes.
