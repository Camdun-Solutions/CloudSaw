# CloudFront distribution does not enforce HTTPS

## Description
The CloudFront distribution has at least one behaviour whose `ViewerProtocolPolicy` is `allow-all`, meaning HTTP requests are served without an HTTPS redirect.

## Risk
Plain-HTTP responses are trivially modified by network-layer attackers (rogue Wi-Fi, ISP middleboxes, BGP hijackers). HSTS and mixed-content protections require HTTPS to be the only protocol the origin honours.

## Detection Logic
The scanner calls `cloudfront:GetDistribution` for each distribution and flags any whose `DefaultCacheBehavior.ViewerProtocolPolicy` or any per-path behaviour is `allow-all`.

## Remediation
Set the viewer protocol policy to `redirect-to-https` (or `https-only` for APIs). Pair this with HSTS via Response Headers Policies.

## Terraform Fix
```hcl
resource "aws_cloudfront_distribution" "site" {
  default_cache_behavior {
    viewer_protocol_policy = "redirect-to-https"
    # ...
  }
}
```

## AWS CLI Fix
```sh
# Distribution config edits go through GetDistributionConfig + UpdateDistribution:
aws cloudfront get-distribution-config --id <id> > config.json
# edit ViewerProtocolPolicy in default and ordered behaviours, then:
aws cloudfront update-distribution --id <id> --if-match <etag> \
  --distribution-config file://config.json
```

## False Positives
None.
