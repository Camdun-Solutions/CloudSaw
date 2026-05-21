# CloudSawScannerRole — least-privilege IAM role used by the bundled scanner.
#
# This module is intentionally tiny: one IAM role, one trust policy, one
# managed-policy attachment. Anything more elaborate (custom inline policies,
# AWS Config rules, EventBridge wiring) belongs in later contracts.
#
# Hard rules baked in here (mirroring Contract 05 §Constraints and CLAUDE.md
# §5 "DO NOT" list):
#   * Principal in the trust policy is the variable trusted_principal_arn —
#     supplied by CloudSaw from sts:GetCallerIdentity, never a wildcard, never
#     user-typed.
#   * Default attached policy is SecurityAudit. ReadOnlyAccess is wired up
#     behind the explicit opt-in variable policy_variant; nothing else is
#     attachable through this module.
#   * No `aws_iam_role_policy` (inline) blocks. No `aws_iam_policy` custom
#     policies. The scanner role gets ONLY a managed-policy attachment.
#   * No `terraform destroy`-amplifying resources (S3 buckets with force_destroy,
#     IAM users, access keys). Destruction of a future version's resources will
#     be handled by an explicit out-of-band tool, not exposed in the app.

locals {
  # Map the policy_variant string to its AWS-managed policy ARN. Keeping the
  # mapping local prevents callers from naming arbitrary managed policies.
  managed_policy_arn = {
    "security_audit"    = "arn:aws:iam::aws:policy/SecurityAudit"
    "read_only_access"  = "arn:aws:iam::aws:policy/ReadOnlyAccess"
  }[var.policy_variant]

  trust_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Effect = "Allow"
        Principal = {
          # Single principal. NO "*". The validator on trusted_principal_arn
          # blocks STS session ARNs and wildcards before we get here.
          AWS = var.trusted_principal_arn
        }
        Action = "sts:AssumeRole"
        Condition = {
          StringEquals = {
            "sts:ExternalId" = var.external_id
          }
        }
      }
    ]
  })
}

resource "aws_iam_role" "scanner" {
  name        = var.role_name
  description = "Read-only role used by CloudSaw to scan this AWS account. Managed by CloudSaw — do not edit by hand."

  assume_role_policy = local.trust_policy

  # Force_detach_policies is OFF on purpose: if the user wires extra policies
  # to this role out-of-band, we want Terraform refusal rather than silent
  # detachment. CloudSaw owns the role's lifecycle through this module.
  force_detach_policies = false

  max_session_duration = 3600 # 1 hour — matches CLAUDE.md §4.3 STS cap.

  tags = {
    "ManagedBy"   = "CloudSaw"
    "Purpose"     = "security-scan"
    "PolicyClass" = var.policy_variant
  }
}

resource "aws_iam_role_policy_attachment" "scanner" {
  role       = aws_iam_role.scanner.name
  policy_arn = local.managed_policy_arn
}
