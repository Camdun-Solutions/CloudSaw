// Bundled-content compile-time index.
//
// Every article that ships with the application is `include_str!`'d here so
// the binary is a self-contained offline source of truth. Adding a new
// article is a two-step change: drop the markdown file under
// `src-tauri/knowledgebase/articles/` and add a new line to
// `BUNDLED_ARTICLES`.
//
// The list MUST be sorted by finding_id so duplicate-detection has a stable
// pairing in error messages and so `list_articles` returns a deterministic
// order (no per-run reshuffling).
//
// CLAUDE.md §4.5 + Contract 08 §Constraints: this content is read-only at
// runtime; the remote-refresh layer never writes to it. A successful refresh
// produces a separate on-disk cache that overrides the bundled set; the
// bundled set is still available as the offline fallback.

/// All bundled (finding_id, markdown) pairs. The order here MUST match the
/// sorted order of finding_ids; the loader validates it at startup so a
/// future maintainer doesn't accidentally introduce a hidden duplicate.
pub const BUNDLED_ARTICLES: &[(&str, &str)] = &[
    (
        "cloudfront-distribution-no-https",
        include_str!("../../knowledgebase/articles/cloudfront-distribution-no-https.md"),
    ),
    (
        "cloudfront-distribution-no-logs",
        include_str!("../../knowledgebase/articles/cloudfront-distribution-no-logs.md"),
    ),
    (
        "cloudtrail-no-log-file-validation",
        include_str!("../../knowledgebase/articles/cloudtrail-no-log-file-validation.md"),
    ),
    (
        "cloudtrail-no-logs-encryption",
        include_str!("../../knowledgebase/articles/cloudtrail-no-logs-encryption.md"),
    ),
    (
        "cloudtrail-not-enabled-globally",
        include_str!("../../knowledgebase/articles/cloudtrail-not-enabled-globally.md"),
    ),
    (
        "ec2-instance-with-public-ip",
        include_str!("../../knowledgebase/articles/ec2-instance-with-public-ip.md"),
    ),
    (
        "ec2-security-group-opens-all-ports-to-all",
        include_str!("../../knowledgebase/articles/ec2-security-group-opens-all-ports-to-all.md"),
    ),
    (
        "ec2-security-group-opens-rdp-to-all",
        include_str!("../../knowledgebase/articles/ec2-security-group-opens-rdp-to-all.md"),
    ),
    (
        "ec2-security-group-opens-ssh-to-all",
        include_str!("../../knowledgebase/articles/ec2-security-group-opens-ssh-to-all.md"),
    ),
    (
        "elb-no-access-logs",
        include_str!("../../knowledgebase/articles/elb-no-access-logs.md"),
    ),
    (
        "elbv2-no-deletion-protection",
        include_str!("../../knowledgebase/articles/elbv2-no-deletion-protection.md"),
    ),
    (
        "iam-password-policy-no-expiration",
        include_str!("../../knowledgebase/articles/iam-password-policy-no-expiration.md"),
    ),
    (
        "iam-password-policy-no-lowercase",
        include_str!("../../knowledgebase/articles/iam-password-policy-no-lowercase.md"),
    ),
    (
        "iam-password-policy-no-minimum-length",
        include_str!("../../knowledgebase/articles/iam-password-policy-no-minimum-length.md"),
    ),
    (
        "iam-password-policy-no-number",
        include_str!("../../knowledgebase/articles/iam-password-policy-no-number.md"),
    ),
    (
        "iam-password-policy-no-reuse-prevention",
        include_str!("../../knowledgebase/articles/iam-password-policy-no-reuse-prevention.md"),
    ),
    (
        "iam-password-policy-no-symbol",
        include_str!("../../knowledgebase/articles/iam-password-policy-no-symbol.md"),
    ),
    (
        "iam-password-policy-no-uppercase",
        include_str!("../../knowledgebase/articles/iam-password-policy-no-uppercase.md"),
    ),
    (
        "iam-root-account-mfa-not-enabled",
        include_str!("../../knowledgebase/articles/iam-root-account-mfa-not-enabled.md"),
    ),
    (
        "iam-root-account-with-access-keys",
        include_str!("../../knowledgebase/articles/iam-root-account-with-access-keys.md"),
    ),
    (
        "iam-user-no-mfa",
        include_str!("../../knowledgebase/articles/iam-user-no-mfa.md"),
    ),
    (
        "iam-user-with-inline-policies",
        include_str!("../../knowledgebase/articles/iam-user-with-inline-policies.md"),
    ),
    (
        "iam-user-with-policies-attached",
        include_str!("../../knowledgebase/articles/iam-user-with-policies-attached.md"),
    ),
    (
        "kms-cmk-rotation-disabled",
        include_str!("../../knowledgebase/articles/kms-cmk-rotation-disabled.md"),
    ),
    (
        "lambda-old-runtime",
        include_str!("../../knowledgebase/articles/lambda-old-runtime.md"),
    ),
    (
        "rds-instance-no-backup",
        include_str!("../../knowledgebase/articles/rds-instance-no-backup.md"),
    ),
    (
        "rds-instance-no-encryption",
        include_str!("../../knowledgebase/articles/rds-instance-no-encryption.md"),
    ),
    (
        "rds-instance-no-minor-version-upgrade",
        include_str!("../../knowledgebase/articles/rds-instance-no-minor-version-upgrade.md"),
    ),
    (
        "rds-instance-publicly-accessible",
        include_str!("../../knowledgebase/articles/rds-instance-publicly-accessible.md"),
    ),
    (
        "s3-bucket-allowing-public-access",
        include_str!("../../knowledgebase/articles/s3-bucket-allowing-public-access.md"),
    ),
    (
        "s3-bucket-no-default-encryption",
        include_str!("../../knowledgebase/articles/s3-bucket-no-default-encryption.md"),
    ),
    (
        "s3-bucket-no-logging",
        include_str!("../../knowledgebase/articles/s3-bucket-no-logging.md"),
    ),
    (
        "s3-bucket-no-mfa-delete",
        include_str!("../../knowledgebase/articles/s3-bucket-no-mfa-delete.md"),
    ),
    (
        "s3-bucket-no-versioning",
        include_str!("../../knowledgebase/articles/s3-bucket-no-versioning.md"),
    ),
    (
        "vpc-default-security-group-with-rules",
        include_str!("../../knowledgebase/articles/vpc-default-security-group-with-rules.md"),
    ),
    (
        "vpc-flow-logs-disabled",
        include_str!("../../knowledgebase/articles/vpc-flow-logs-disabled.md"),
    ),
];

/// Pinned compiled-in mappings document. The runtime mappings registry
/// deserializes this lazily on first use and caches the result.
pub const BUNDLED_MAPPINGS_JSON: &str =
    include_str!("../../knowledgebase/mappings.json");

/// Build-time version stamp embedded into `RefreshSettings.last_applied_at`
/// when the bundled content is the active set. Versioning the bundle by the
/// crate version is sufficient — the bundle ships with the binary and
/// can't drift independently.
pub const BUNDLED_VERSION: &str = env!("CARGO_PKG_VERSION");
