# CloudFront distribution has no access logs

## Description
The CloudFront distribution does not deliver access logs to an S3 bucket. Without access logs, there is no record of who fetched which URLs at the edge.

## Risk
CloudFront access logs are essential for content-piracy investigations, abuse-rate analysis, and detection of credential-stuffing attempts hitting cached endpoints. Without them, the trail stops at the cache hit.

## Detection Logic
The scanner inspects each distribution's `Logging.Enabled` flag and flags distributions where it is `false`.

## Remediation
Enable real-time logs (Kinesis Data Streams) for high-traffic distributions or standard logs (S3 delivery) for routine workloads. Keep the log bucket private and lifecycle-managed.

## Terraform Fix
```hcl
resource "aws_cloudfront_distribution" "site" {
  logging_config {
    bucket          = "${aws_s3_bucket.logs.id}.s3.amazonaws.com"
    include_cookies = false
    prefix          = "cloudfront/${var.name}/"
  }
}
```

## AWS CLI Fix
Use the `GetDistributionConfig` + `UpdateDistribution` workflow to set the `Logging` block on the distribution.

## False Positives
Distributions used exclusively for static-site delivery of marketing pages with no auth flow may tolerate "no logs" with documented acceptance — but the cost is small relative to incident-response value.
