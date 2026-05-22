# IAM user has policies attached directly

## Description
The IAM user has a managed policy attached directly to the user rather than through a group. Direct attachments make it harder to reason about least privilege at scale because the source of truth diffuses across hundreds of user records.

## Risk
Direct user-to-policy attachments inhibit centralised review and make membership-driven access patterns (joiner/mover/leaver) much harder to audit. Over time, users accumulate stale permissions.

## Detection Logic
For each IAM user, the scanner calls `iam:ListAttachedUserPolicies` and flags any user with attached managed policies.

## Remediation
Move attached policies onto groups (or, ideally, switch the account to IAM Identity Center and assign permission sets via groups). Each user should obtain their permissions from group membership, not direct attachment.

## Terraform Fix
```hcl
resource "aws_iam_group" "engineers" {
  name = "engineers"
}

resource "aws_iam_group_policy_attachment" "engineers_readonly" {
  group      = aws_iam_group.engineers.name
  policy_arn = "arn:aws:iam::aws:policy/ReadOnlyAccess"
}

resource "aws_iam_user_group_membership" "alice" {
  user   = "alice"
  groups = [aws_iam_group.engineers.name]
}
```

## AWS CLI Fix
```sh
aws iam list-attached-user-policies --user-name <user>
aws iam detach-user-policy --user-name <user> --policy-arn <arn>
aws iam attach-group-policy --group-name <group> --policy-arn <arn>
aws iam add-user-to-group --user-name <user> --group-name <group>
```

## False Positives
None for steady-state. During migrations, expect transient direct attachments while groups are being assembled — re-run the scan once the migration completes.
