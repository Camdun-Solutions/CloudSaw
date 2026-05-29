// ScoutSuite JSON → normalized in-memory model.
//
// Pure functions only. Given a `serde_json::Value` (already deserialized
// from `raw-scout.json`) and the scan's account_id, this module produces a
// `ParsedScoutOutput` that the storage layer feeds into SQLite.
//
// CLAUDE.md §4.5 + Contract 07 §Constraints: the parser is pure — identical
// input JSON yields identical output (sort order, field values). Time-stamp
// bookkeeping happens one layer up, in the storage apply step, so the parser
// itself never depends on the wall clock.
//
// What ScoutSuite emits (the input shape, abridged):
//
//   {
//     "account_id": "111122223333",
//     "provider_code": "aws",
//     "services": {
//       "iam": {
//         "findings": {
//           "iam-password-policy-no-minimum-length": {
//             "description": "...", "rationale": "...",
//             "dashboard_name": "Password policy",
//             "path": "iam.password_policy",
//             "level": "danger",       // or "warning", "info", ...
//             "items": ["iam.password_policy.MinimumPasswordLength"],
//             "checked_items": 1, "flagged_items": 1,
//             "service": "iam"
//           }
//         },
//         ...
//       },
//       ...
//     }
//   }
//
// Findings whose `flagged_items` is 0 are emitted by ScoutSuite as
// informational "we checked nothing was wrong" markers. We still record
// them so the UI can show coverage, but their normalized severity falls to
// Informational regardless of the raw level.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::types::Severity;

/// Output of `parse_scoutsuite`. Keep ordering stable (BTree-derived) so
/// the apply step is reproducible — see `apply_parsed` for the rationale.
#[derive(Debug, Clone)]
pub struct ParsedScoutOutput {
    /// Account ID echoed by the scanner. The caller compares this against
    /// the scan's stored account_id before touching the database.
    pub account_id: Option<String>,
    /// Every service the scanner walked, even those with zero findings.
    /// Used by the resolution step: a finding from a prior scan is only
    /// marked resolved if its `service` was actually covered by the
    /// current scan.
    pub services_scanned: BTreeSet<String>,
    /// One entry per (service, rule_key). Sorted by rule_key within
    /// service, then by service name, for deterministic ordering.
    pub findings: Vec<ParsedFinding>,
    /// Counters for telemetry / ParseSummary.
    pub unknown_severity_count: usize,
    pub unknown_type_count: usize,
}

/// One finding row in normalized form, ready to apply.
#[derive(Debug, Clone)]
pub struct ParsedFinding {
    pub rule_key: String,
    pub raw_type: String,
    pub service: String,
    pub severity: Severity,
    pub description: String,
    pub rationale: Option<String>,
    pub dashboard_name: Option<String>,
    pub resource_path_pattern: Option<String>,
    pub checked_items: i64,
    pub flagged_items: i64,
    /// Resource paths from the finding's `items` array, deduped and
    /// sorted. Each entry already carries the `invalid` flag from the
    /// path-shape check.
    pub resources: Vec<ParsedResource>,
}

#[derive(Debug, Clone)]
pub struct ParsedResource {
    pub resource_path: String,
    pub invalid: bool,
    // PR #82 — identity fields walked out of the deepest "resource
    // entity" ancestor of the ScoutSuite item path. All optional —
    // they're empty for findings whose path doesn't land on a dict
    // with `name`/`arn`/`id` scalars (e.g. password_policy globals).
    pub resource_name: Option<String>,
    pub resource_arn: Option<String>,
    pub resource_id_value: Option<String>,
    /// Forward-compat: every other scalar field captured at the
    /// resource entity level (creation timestamp, key counts, tag
    /// map, etc.). Serialized to JSON when persisted so the UI can
    /// render every key without forcing a schema change for each
    /// new attribute ScoutSuite emits.
    pub attributes: BTreeMap<String, serde_json::Value>,
}

/// The set of finding-type prefixes CloudSaw "recognizes" in v1. Unknown
/// types are still stored verbatim — this set only drives the
/// unknown_type_count telemetry counter so the UI can offer to surface
/// them. The list is intentionally short; later contracts (Contract 08's
/// knowledge base) will expand it.
const RECOGNIZED_TYPE_PREFIXES: &[&str] = &[
    "iam-",
    "s3-",
    "ec2-",
    "vpc-",
    "rds-",
    "cloudtrail-",
    "cloudfront-",
    "cloudwatch-",
    "cloudformation-",
    "acm-",
    "kms-",
    "elb-",
    "elbv2-",
    "lambda-",
    "logs-",
    "sns-",
    "sqs-",
    "ses-",
    "secretsmanager-",
    "ssm-",
    "config-",
    "redshift-",
    "route53-",
    "dynamodb-",
    "guardduty-",
    "emr-",
    "efs-",
    "elasticache-",
    "awslambda-",
];

/// Walk the ScoutSuite output tree and produce the normalized model.
///
/// This function never reads time and never touches the database. It is
/// the only place that knows the ScoutSuite output shape — the rest of
/// the module operates on `ParsedScoutOutput`.
pub fn parse_scoutsuite(json: &Value) -> ParsedScoutOutput {
    let account_id = json
        .get("account_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let mut services_scanned = BTreeSet::new();
    let mut findings_by_key: std::collections::BTreeMap<(String, String), ParsedFinding> =
        std::collections::BTreeMap::new();
    let mut unknown_severity_count = 0usize;
    let mut unknown_type_count = 0usize;

    let services = json.get("services").and_then(|v| v.as_object());
    if let Some(services) = services {
        for (service_name, service_value) in services {
            services_scanned.insert(service_name.clone());

            let findings_obj = service_value.get("findings").and_then(|v| v.as_object());
            let Some(findings_obj) = findings_obj else {
                continue;
            };
            for (rule_key, finding_value) in findings_obj {
                let raw_level = finding_value
                    .get("level")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let parsed_level = if raw_level.is_empty() {
                    None
                } else {
                    Severity::from_raw_level(raw_level)
                };

                let flagged_items = finding_value
                    .get("flagged_items")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                let checked_items = finding_value
                    .get("checked_items")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);

                let severity = match parsed_level {
                    Some(level) => {
                        // A finding with zero flagged items is informational
                        // even if the rule's default level is higher — the
                        // scanner is reporting coverage, not a hit.
                        if flagged_items == 0 {
                            Severity::Informational
                        } else {
                            level
                        }
                    }
                    None => {
                        if !raw_level.is_empty() {
                            unknown_severity_count += 1;
                            eprintln!(
                                "findings: unknown severity level '{}' for rule '{}'; mapping to informational",
                                raw_level, rule_key
                            );
                        }
                        Severity::Informational
                    }
                };

                let description = finding_value
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let rationale = finding_value
                    .get("rationale")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let dashboard_name = finding_value
                    .get("dashboard_name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let resource_path_pattern = finding_value
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                // ScoutSuite emits the service on each finding too; prefer
                // it when present, fall back to the containing key.
                let service = finding_value
                    .get("service")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| service_name.clone());

                // Resource items. We dedupe and sort to keep the apply step
                // deterministic (BTreeSet on the way in, Vec on the way out).
                let mut resource_set: BTreeSet<String> = BTreeSet::new();
                if let Some(items) = finding_value.get("items").and_then(|v| v.as_array()) {
                    for item in items {
                        if let Some(path) = item.as_str() {
                            if !path.is_empty() {
                                resource_set.insert(path.to_string());
                            }
                        }
                    }
                }
                let resources: Vec<ParsedResource> = resource_set
                    .into_iter()
                    .map(|path| {
                        let invalid = !looks_like_valid_resource_path(&path);
                        // PR #82 — walk the deepest resource-entity
                        // ancestor of the item path. `services` is the
                        // root of the ScoutSuite output tree; the path's
                        // first segment is the service (`iam`, `s3`,
                        // …), and we walk down from there to find the
                        // last dict carrying `name`/`arn`/`id` scalars.
                        // Failure modes (path doesn't exist, walk lands
                        // on a non-object, no identity fields anywhere
                        // on the path) all surface as empty optionals,
                        // and the UI falls back to the raw path.
                        let (resource_name, resource_arn, resource_id_value, attributes) =
                            walk_resource_entity(Some(services), &path);
                        ParsedResource {
                            resource_path: path,
                            invalid,
                            resource_name,
                            resource_arn,
                            resource_id_value,
                            attributes,
                        }
                    })
                    .collect();

                let is_recognized_type = RECOGNIZED_TYPE_PREFIXES
                    .iter()
                    .any(|p| rule_key.starts_with(p));
                if !is_recognized_type {
                    unknown_type_count += 1;
                }

                let parsed = ParsedFinding {
                    rule_key: rule_key.clone(),
                    raw_type: rule_key.clone(),
                    service,
                    severity,
                    description,
                    rationale,
                    dashboard_name,
                    resource_path_pattern,
                    checked_items,
                    flagged_items,
                    resources,
                };
                findings_by_key.insert((service_name.clone(), rule_key.clone()), parsed);
            }
        }
    }

    let findings: Vec<ParsedFinding> = findings_by_key.into_values().collect();

    ParsedScoutOutput {
        account_id,
        services_scanned,
        findings,
        unknown_severity_count,
        unknown_type_count,
    }
}

/// Stable finding identity. Hash inputs are the partition key and the
/// rule_key — a re-scan of the same account producing the same rule yields
/// the same finding_id, so storage upserts rather than duplicates
/// (Contract 07 §Constraints).
pub fn finding_id_for(aws_account_id: &str, rule_key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(aws_account_id.as_bytes());
    hasher.update(b":");
    hasher.update(rule_key.as_bytes());
    hex::encode(hasher.finalize())
}

/// PR #82 — Walk a ScoutSuite item path through the `services` tree and
/// return the resource entity's identity fields + scalars. The "resource
/// entity" is the deepest dict along the path that carries any of
/// `name` / `arn` / `id` (or their PascalCase variants) — typically the
/// `iam.users.<uid>` / `s3.buckets.<id>` / `ec2.regions.<r>.instances.
/// <iid>` level. Returns nulls when no entity is found (e.g.
/// `iam.password_policy.MaxPasswordAge` walks to a flat config block
/// with no identifying scalars).
///
/// The fourth tuple slot is every OTHER scalar field at the entity level
/// (CreateDate, AccessKeys count, etc.) so the UI can render every key
/// without forcing a schema change for each new attribute ScoutSuite
/// emits.
fn walk_resource_entity(
    services: Option<&serde_json::Map<String, Value>>,
    path: &str,
) -> (
    Option<String>,
    Option<String>,
    Option<String>,
    BTreeMap<String, Value>,
) {
    let services = match services {
        Some(s) => s,
        None => return (None, None, None, BTreeMap::new()),
    };
    let parts: Vec<&str> = path.split('.').collect();
    let mut deepest_obj: Option<&serde_json::Map<String, Value>> = None;
    let mut walker: Option<&Value> = None;
    for (idx, part) in parts.iter().enumerate() {
        let next = if idx == 0 {
            services.get(*part)
        } else if let Some(v) = walker {
            v.get(*part)
        } else {
            None
        };
        let next_val = match next {
            Some(v) => v,
            None => break,
        };
        if let Some(obj) = next_val.as_object() {
            if has_identity_scalar(obj) {
                deepest_obj = Some(obj);
            }
        }
        walker = Some(next_val);
    }

    let obj = match deepest_obj {
        Some(o) => o,
        None => return (None, None, None, BTreeMap::new()),
    };
    let name = pick_string(
        obj,
        &[
            "name",
            "Name",
            "UserName",
            "RoleName",
            "PolicyName",
            "GroupName",
            "BucketName",
        ],
    );
    let arn = pick_string(obj, &["arn", "Arn", "ARN"]);
    let id_value = pick_string(obj, &["id", "Id", "ID"]);
    let mut attributes: BTreeMap<String, Value> = BTreeMap::new();
    // PR #82 — copy every other scalar (string / number / bool) at the
    // entity level, skipping the three identity columns we already
    // captured. Skip nested arrays/objects to keep the JSON payload
    // small; the frontend can render those later if needed.
    for (k, v) in obj {
        let key = k.as_str();
        if matches!(
            key,
            "name"
                | "Name"
                | "UserName"
                | "RoleName"
                | "PolicyName"
                | "GroupName"
                | "BucketName"
                | "arn"
                | "Arn"
                | "ARN"
                | "id"
                | "Id"
                | "ID"
        ) {
            continue;
        }
        match v {
            Value::Null => {}
            Value::Bool(_) | Value::Number(_) | Value::String(_) => {
                attributes.insert(k.clone(), v.clone());
            }
            _ => {}
        }
    }
    (name, arn, id_value, attributes)
}

fn has_identity_scalar(obj: &serde_json::Map<String, Value>) -> bool {
    const KEYS: &[&str] = &[
        "name",
        "Name",
        "UserName",
        "RoleName",
        "PolicyName",
        "GroupName",
        "BucketName",
        "arn",
        "Arn",
        "ARN",
        "id",
        "Id",
        "ID",
    ];
    KEYS.iter()
        .any(|k| matches!(obj.get(*k), Some(v) if v.is_string()))
}

fn pick_string(obj: &serde_json::Map<String, Value>, candidates: &[&str]) -> Option<String> {
    for k in candidates {
        if let Some(s) = obj.get(*k).and_then(|v| v.as_str()) {
            if !s.is_empty() {
                return Some(s.to_string());
            }
        }
    }
    None
}

/// Shape-check for a resource path. We don't need a strict ARN parser —
/// ScoutSuite paths are dotted expressions, not ARNs — but we do flag
/// values that obviously can't be a valid path: empty after trim, ASCII
/// control characters, or absurd length. The QA edge case "malformed
/// resource ARN → row stored, flagged invalid" only requires that we
/// notice and tag, not that we recover meaning.
fn looks_like_valid_resource_path(s: &str) -> bool {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.len() > 4096 {
        return false;
    }
    if trimmed.chars().any(|c| c.is_control()) {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn severity_maps_scoutsuite_levels_to_normalized_scale() {
        assert_eq!(Severity::from_raw_level("danger"), Some(Severity::High));
        assert_eq!(Severity::from_raw_level("warning"), Some(Severity::Medium));
        assert_eq!(
            Severity::from_raw_level("info"),
            Some(Severity::Informational)
        );
        // Unknown
        assert!(Severity::from_raw_level("uhoh").is_none());
    }

    #[test]
    fn parser_extracts_finding_with_resources() {
        let input = json!({
            "account_id": "111122223333",
            "services": {
                "iam": {
                    "findings": {
                        "iam-password-policy-no-minimum-length": {
                            "description": "No minimum password length",
                            "rationale": "Stronger passwords reduce risk.",
                            "dashboard_name": "Password policy",
                            "path": "iam.password_policy",
                            "level": "danger",
                            "items": ["iam.password_policy.MinimumPasswordLength"],
                            "checked_items": 1,
                            "flagged_items": 1,
                            "service": "iam"
                        }
                    }
                }
            }
        });
        let parsed = parse_scoutsuite(&input);
        assert_eq!(parsed.account_id.as_deref(), Some("111122223333"));
        assert!(parsed.services_scanned.contains("iam"));
        assert_eq!(parsed.findings.len(), 1);
        let f = &parsed.findings[0];
        assert_eq!(f.rule_key, "iam-password-policy-no-minimum-length");
        assert_eq!(f.severity, Severity::High);
        assert_eq!(f.flagged_items, 1);
        assert_eq!(f.resources.len(), 1);
        assert!(!f.resources[0].invalid);
    }

    #[test]
    fn parser_flags_invalid_resource_paths() {
        let input = json!({
            "account_id": "111122223333",
            "services": {
                "ec2": {
                    "findings": {
                        "ec2-default-security-group-in-use": {
                            "description": "...",
                            "level": "warning",
                            "items": ["bad\u{0000}path"],
                            "checked_items": 1,
                            "flagged_items": 1
                        }
                    }
                }
            }
        });
        let parsed = parse_scoutsuite(&input);
        assert_eq!(parsed.findings.len(), 1);
        let f = &parsed.findings[0];
        assert_eq!(f.resources.len(), 1);
        assert!(f.resources[0].invalid);
    }

    #[test]
    fn parser_demotes_zero_flagged_findings_to_informational() {
        let input = json!({
            "account_id": "111122223333",
            "services": {
                "iam": {
                    "findings": {
                        "iam-root-account-no-mfa": {
                            "description": "...",
                            "level": "danger",
                            "items": [],
                            "checked_items": 1,
                            "flagged_items": 0
                        }
                    }
                }
            }
        });
        let parsed = parse_scoutsuite(&input);
        let f = &parsed.findings[0];
        assert_eq!(f.severity, Severity::Informational);
    }

    #[test]
    fn parser_records_unknown_severity_as_informational_with_count() {
        let input = json!({
            "account_id": "111122223333",
            "services": {
                "iam": {
                    "findings": {
                        "iam-something-new": {
                            "description": "...",
                            "level": "unrecognized-level",
                            "items": ["iam.something"],
                            "checked_items": 1,
                            "flagged_items": 1
                        }
                    }
                }
            }
        });
        let parsed = parse_scoutsuite(&input);
        assert_eq!(parsed.findings.len(), 1);
        assert_eq!(parsed.findings[0].severity, Severity::Informational);
        assert_eq!(parsed.unknown_severity_count, 1);
    }

    #[test]
    fn parser_preserves_unknown_rule_type() {
        let input = json!({
            "account_id": "111122223333",
            "services": {
                "newservice": {
                    "findings": {
                        "newservice-novel-finding-type": {
                            "description": "...",
                            "level": "warning",
                            "items": ["newservice.thing"],
                            "checked_items": 1,
                            "flagged_items": 1
                        }
                    }
                }
            }
        });
        let parsed = parse_scoutsuite(&input);
        assert_eq!(parsed.findings.len(), 1);
        assert_eq!(parsed.findings[0].raw_type, "newservice-novel-finding-type");
        assert_eq!(parsed.unknown_type_count, 1);
    }

    #[test]
    fn parser_is_deterministic_for_identical_input() {
        let input = json!({
            "account_id": "111122223333",
            "services": {
                "iam": {
                    "findings": {
                        "iam-a": {
                            "level": "danger",
                            "items": ["iam.a.1", "iam.a.2"],
                            "checked_items": 5,
                            "flagged_items": 2,
                            "description": "a"
                        },
                        "iam-b": {
                            "level": "warning",
                            "items": ["iam.b.1"],
                            "checked_items": 3,
                            "flagged_items": 1,
                            "description": "b"
                        }
                    }
                }
            }
        });
        let p1 = parse_scoutsuite(&input);
        let p2 = parse_scoutsuite(&input);
        assert_eq!(p1.findings.len(), p2.findings.len());
        for (a, b) in p1.findings.iter().zip(p2.findings.iter()) {
            assert_eq!(a.rule_key, b.rule_key);
            assert_eq!(a.severity, b.severity);
            assert_eq!(a.flagged_items, b.flagged_items);
            let ar: Vec<&str> = a
                .resources
                .iter()
                .map(|r| r.resource_path.as_str())
                .collect();
            let br: Vec<&str> = b
                .resources
                .iter()
                .map(|r| r.resource_path.as_str())
                .collect();
            assert_eq!(ar, br);
        }
    }

    #[test]
    fn finding_id_stable_across_calls() {
        let a = finding_id_for("111122223333", "iam-foo");
        let b = finding_id_for("111122223333", "iam-foo");
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
    }

    #[test]
    fn finding_id_differs_across_accounts_and_rules() {
        assert_ne!(
            finding_id_for("111122223333", "iam-foo"),
            finding_id_for("999988887777", "iam-foo"),
        );
        assert_ne!(
            finding_id_for("111122223333", "iam-foo"),
            finding_id_for("111122223333", "iam-bar"),
        );
    }
}
