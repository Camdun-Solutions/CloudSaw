# Root account has active access keys

## Description
The AWS account root user has at least one active access key. Programmatic root credentials are the most dangerous credentials in AWS — they cannot be limited by IAM and are not bound to MFA in API calls by default.

## Risk
Any leak of a root access key gives the attacker full account control, with no policy backstop. Public-repo scraping, accidental commits, and CI exfiltration all routinely catch root keys.

## Detection Logic
The scanner reads `iam:GetAccountSummary` and flags any account whose `AccountAccessKeysPresent` is greater than 0.

## Remediation
Delete every root access key. If an automation depends on root, that automation is wrong — replace it with an IAM role assumed by a least-privilege user.

## Terraform Fix
Root access keys cannot be removed by Terraform (the root user is not a Terraform-managed resource). Sign in as root and delete them via the Security Credentials console.

## AWS CLI Fix
```sh
# Sign-in as root, then:
aws iam list-access-keys --user-name root   # for visibility
aws iam delete-access-key --user-name root --access-key-id <key-id>
```

## False Positives
None. AWS itself recommends zero active root access keys.
