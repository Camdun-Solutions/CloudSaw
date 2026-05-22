# VPC Flow Logs are disabled

## Description
The VPC does not have Flow Logs enabled. Without Flow Logs, there is no traffic-level audit trail to draw on during incident response.

## Risk
During a breach investigation, the inability to enumerate which IPs talked to which workloads at the network layer prevents both scoping and attribution. Flow Logs are the cloud equivalent of NetFlow / IPFIX and are foundational to any incident-response runbook.

## Detection Logic
The scanner calls `ec2:DescribeFlowLogs` and flags any VPC with no active Flow Log of resource-type `VPC`.

## Remediation
Enable Flow Logs on every VPC with delivery to either CloudWatch Logs or an S3 bucket. Choose `ALL` traffic types in lower-volume accounts; in high-volume accounts capture `REJECT` only to bound costs, with the option to widen during incidents.

## Terraform Fix
```hcl
resource "aws_flow_log" "vpc" {
  vpc_id          = aws_vpc.main.id
  traffic_type    = "ALL"
  log_destination_type = "s3"
  log_destination = aws_s3_bucket.flow_logs.arn
}
```

## AWS CLI Fix
```sh
aws ec2 create-flow-logs \
  --resource-ids <vpc-id> \
  --resource-type VPC \
  --traffic-type ALL \
  --log-destination-type s3 \
  --log-destination <s3-bucket-arn>
```

## False Positives
None. The cost of capturing flow logs at the `REJECT`-only level is small; the cost of not having them during an incident is much larger.
