# Security group opens all ports to the world

## Description
The security group has at least one ingress rule with a `0.0.0.0/0` source and a port range that spans all ports (or the protocol is `-1`, meaning all protocols). Any resource using this security group is fully exposed to the public internet.

## Risk
Wide-open security groups are routinely scanned and abused within minutes of being created. They expose every service the instance hosts — admin panels, databases, monitoring endpoints — to opportunistic attack.

## Detection Logic
The scanner calls `ec2:DescribeSecurityGroups` and flags any group whose ingress rules combine a `0.0.0.0/0` (or `::/0`) source with `IpProtocol = "-1"` or a port range that covers 0-65535.

## Remediation
Replace the rule with narrowly scoped rules: per-service ports (e.g. 443 only for web), per-source CIDRs (e.g. corporate-VPN ranges) or referenced security-group IDs (e.g. only the load balancer's security group). Move administrative access behind Session Manager rather than open SSH/RDP.

## Terraform Fix
```hcl
resource "aws_security_group" "web" {
  name = "web"

  ingress {
    from_port   = 443
    to_port     = 443
    protocol    = "tcp"
    cidr_blocks = ["0.0.0.0/0"]
  }

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }
}
```

## AWS CLI Fix
```sh
# Identify the offending rule:
aws ec2 describe-security-groups --group-ids <sg-id>
# Revoke it:
aws ec2 revoke-security-group-ingress \
  --group-id <sg-id> --protocol -1 --port -1 --cidr 0.0.0.0/0
```

## False Positives
Public load-balancer security groups that intentionally accept 443 from `0.0.0.0/0` are correctly flagged only when they also open additional ports beyond 443. Review the rule list carefully before dismissing.
