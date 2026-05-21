# Outputs CloudSaw reads after `terraform apply` succeeds.
#
# `role_arn` is the canonical scanner-role ARN; later contracts (Contract 06)
# call sts:AssumeRole against it. `trust_policy_json` is captured so the
# acceptance-criteria check ("trust policy principal equals the caller's ARN")
# can be made against the actual rendered policy, not the input variable.

output "role_arn" {
  description = "ARN of the CloudSawScannerRole IAM role."
  value       = aws_iam_role.scanner.arn
}

output "role_name" {
  description = "Name of the CloudSawScannerRole IAM role."
  value       = aws_iam_role.scanner.name
}

output "policy_variant" {
  description = "Which managed policy was attached."
  value       = var.policy_variant
}

output "trust_policy_json" {
  description = "The rendered trust policy. Used by CloudSaw to verify principal at the acceptance-criteria layer."
  value       = aws_iam_role.scanner.assume_role_policy
}
