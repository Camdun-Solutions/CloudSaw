# EC2 instance has a public IP

## Description
The EC2 instance has a public IP address. Combined with permissive security-group rules, this exposes the instance directly to the internet.

## Risk
Public-IP instances are scanned constantly by mass-scanning tooling. Any misconfigured service, default credential, or vulnerable build becomes a discoverable target. Most workloads do not need a public IP — a NAT gateway covers outbound needs, and an Application Load Balancer covers inbound needs more safely.

## Detection Logic
The scanner calls `ec2:DescribeInstances` and flags any instance whose `PublicIpAddress` is non-empty.

## Remediation
Move the instance into a private subnet behind a NAT gateway for outbound, and front it with an ALB or NLB for inbound. For administrative access, use Session Manager rather than direct SSH/RDP. Re-architect single-instance workloads as auto-scaling groups behind load balancers when feasible.

## Terraform Fix
```hcl
resource "aws_instance" "app" {
  subnet_id                   = aws_subnet.private.id
  associate_public_ip_address = false
  # ...
}
```

## AWS CLI Fix
Moving an existing instance off a public IP requires either disassociating the EIP and stopping the instance, or replacing the instance entirely:
```sh
# If the public IP comes from an Elastic IP:
aws ec2 disassociate-address --association-id <assoc-id>
aws ec2 release-address --allocation-id <alloc-id>
# Otherwise rebuild the instance in a private subnet.
```

## False Positives
Bastion hosts, externally-reachable VPN concentrators, and instances behind anti-DDoS edge services may legitimately have public IPs. Document each exception so the audit trail is unambiguous.
