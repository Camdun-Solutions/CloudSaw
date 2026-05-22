# Elastic Load Balancer has no access logs

## Description
The Classic ELB does not have access logging enabled to deliver per-request records to S3. Without these logs, there is no per-connection record of client behaviour at the load-balancer layer.

## Risk
ELB access logs are essential to correlate application-layer events with the source IP that produced them and to detect abusive scanners or brute-force attempts targeting the load balancer.

## Detection Logic
The scanner calls `elasticloadbalancing:DescribeLoadBalancerAttributes` and flags any classic ELB whose `AccessLog.Enabled` is `false`.

## Remediation
Enable access logs and deliver to a dedicated log bucket. For new workloads, migrate from Classic ELB to Application Load Balancer where possible.

## Terraform Fix
```hcl
resource "aws_elb" "primary" {
  access_logs {
    bucket   = aws_s3_bucket.logs.id
    interval = 5
    enabled  = true
  }
}
```

## AWS CLI Fix
```sh
aws elb modify-load-balancer-attributes \
  --load-balancer-name <name> \
  --load-balancer-attributes "{\"AccessLog\":{\"Enabled\":true,\"S3BucketName\":\"<bucket>\",\"EmitInterval\":5}}"
```

## False Positives
None.
