# Default VPC security group has rules

## Description
The VPC's default security group contains ingress or egress rules. The default security group is automatically attached to any ENI that does not specify a group; if it has rules, those rules unintentionally apply to anything created without an explicit group.

## Risk
A workload accidentally launched without an explicit security group will inherit whatever the default group allows. This is one of the highest-friction sources of "unintended exposure" findings during audits.

## Detection Logic
The scanner enumerates each default security group and flags any with non-empty `IpPermissions` or `IpPermissionsEgress`.

## Remediation
Remove every rule from the default security group on every VPC, and treat the empty default group as "deny everything if you forgot to pick a group." Explicit groups are mandatory for every workload.

## Terraform Fix
```hcl
resource "aws_default_security_group" "default" {
  vpc_id = aws_vpc.main.id
  # No ingress or egress blocks => no rules.
}
```

## AWS CLI Fix
```sh
# Revoke every rule on the default group of each VPC:
aws ec2 revoke-security-group-ingress \
  --group-id <default-sg-id> --ip-permissions <json>
aws ec2 revoke-security-group-egress \
  --group-id <default-sg-id> --ip-permissions <json>
```

## False Positives
None. The default group exists as a fail-safe; rules on it defeat the safety.
