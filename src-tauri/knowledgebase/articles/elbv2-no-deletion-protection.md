# Application/Network Load Balancer has no deletion protection

## Description
The ALB or NLB has `deletion_protection.enabled = false`. Any operator with the matching IAM permission can delete the load balancer in a single API call.

## Risk
Accidental deletion of a production load balancer is a high-severity outage event. Deletion protection is a cheap guardrail that converts a single misclick into a deliberate two-step operation.

## Detection Logic
The scanner calls `elasticloadbalancing:DescribeLoadBalancerAttributes` (v2) and flags any ALB/NLB whose `deletion_protection.enabled` is `false`.

## Remediation
Enable deletion protection on all production load balancers. For non-production, document the choice in the IaC repository.

## Terraform Fix
```hcl
resource "aws_lb" "primary" {
  enable_deletion_protection = true
  # ...
}
```

## AWS CLI Fix
```sh
aws elbv2 modify-load-balancer-attributes \
  --load-balancer-arn <arn> \
  --attributes Key=deletion_protection.enabled,Value=true
```

## False Positives
Genuinely ephemeral test or PR-preview load balancers can leave deletion protection off intentionally.
