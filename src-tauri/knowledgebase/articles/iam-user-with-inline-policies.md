# IAM user has inline policies attached

## Description
The IAM user has at least one inline policy. Inline policies are not reusable, are easy to lose track of, and drift out of sync with intended grants because they don't have a single named entity owning them.

## Risk
Inline policies make audits harder: a permission grant lives "inside" the user rather than as a named, versioned managed policy. This makes it easy to grant excessive permissions and lose visibility on who has what.

## Detection Logic
For each IAM user, the scanner calls `iam:ListUserPolicies` and flags any user with one or more inline policies.

## Remediation
Convert the inline policies to managed policies and attach them to a group (or, better, attach them to a role assumed by the user). Apply least-privilege when migrating — many inline policies accumulated grants over time that are no longer needed.

## Terraform Fix
```hcl
resource "aws_iam_policy" "named_policy" {
  name   = "ReadOnlyBilling"
  policy = jsonencode({/* ... */})
}

resource "aws_iam_group_policy_attachment" "users_billing_read" {
  group      = "billing-readers"
  policy_arn = aws_iam_policy.named_policy.arn
}
```

## AWS CLI Fix
```sh
aws iam list-user-policies --user-name <user>
# For each policy:
aws iam get-user-policy --user-name <user> --policy-name <policy>
aws iam create-policy --policy-name <policy> --policy-document file://policy.json
aws iam attach-user-policy --user-name <user> --policy-arn <new-arn>
aws iam delete-user-policy --user-name <user> --policy-name <policy>
```

## False Positives
Short-lived inline policies attached during incident response are acceptable in the very short term — the underlying assertion still stands: the policy should be reviewed and either promoted to a managed policy or removed.
