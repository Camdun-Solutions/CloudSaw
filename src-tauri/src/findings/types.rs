// Public data types crossing the IPC boundary for the findings module.
//
// CLAUDE.md §4.1: IPC payloads are plain serializable structs — no AWS SDK
// types, no credential-bearing types. Every field below is a primitive or a
// deliberately enumerated tag.
//
// The five-tier severity scale (Critical/High/Medium/Low/Informational) is
// the normalized form Contract 07 §Constraints requires. ScoutSuite emits
// the two-tier `danger`/`warning` plus an occasional `info` — the parser
// maps those into this enum, and an unrecognized level logs a warning and
// maps to Informational rather than failing (Contract 07 §Edge Cases).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Normalized severity. See `Severity::from_raw_level` for the ScoutSuite
/// mapping. Stored in SQLite as the lowercase string form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
    Informational,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Critical => "critical",
            Severity::High => "high",
            Severity::Medium => "medium",
            Severity::Low => "low",
            Severity::Informational => "informational",
        }
    }

    /// Parse a stored severity string back to the enum. Unknown values are
    /// rejected — storage is never written with anything other than the
    /// normalized form, so an unknown value indicates corruption rather
    /// than forward-compat. Callers convert to Informational with a
    /// warning log; see `storage::row_to_finding`.
    pub fn from_storage(s: &str) -> Option<Severity> {
        match s {
            "critical" => Some(Severity::Critical),
            "high" => Some(Severity::High),
            "medium" => Some(Severity::Medium),
            "low" => Some(Severity::Low),
            "informational" => Some(Severity::Informational),
            _ => None,
        }
    }

    /// Map a raw ScoutSuite "level" string to the normalized scale.
    /// Returns `None` for unrecognized inputs so the caller can log the
    /// raw value before falling back to Informational. Per Contract 07
    /// §Constraints: "Unknown severities map to `informational` with a
    /// logged warning."
    pub fn from_raw_level(raw: &str) -> Option<Severity> {
        match raw.to_ascii_lowercase().as_str() {
            // ScoutSuite's two-tier scheme.
            "danger" => Some(Severity::High),
            "warning" => Some(Severity::Medium),
            // ScoutSuite + future providers occasionally emit these as a
            // hint; treat them charitably.
            "critical" => Some(Severity::Critical),
            "high" => Some(Severity::High),
            "medium" => Some(Severity::Medium),
            "low" => Some(Severity::Low),
            "info" | "informational" | "notice" => Some(Severity::Informational),
            _ => None,
        }
    }
}

/// Finding lifecycle status. `Open` means the finding was observed in its
/// last-seen scan and has not been resolved by a subsequent scan. `Resolved`
/// means a later scan that covered the relevant service no longer reported
/// the finding — the row is retained for trend analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingStatus {
    Open,
    Resolved,
}

impl FindingStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            FindingStatus::Open => "open",
            FindingStatus::Resolved => "resolved",
        }
    }

    pub fn from_storage(s: &str) -> Option<FindingStatus> {
        match s {
            "open" => Some(FindingStatus::Open),
            "resolved" => Some(FindingStatus::Resolved),
            _ => None,
        }
    }
}

/// One aggregated finding row, ready to render in the UI.
///
/// `aws_account_id` is the full 12-digit value (CLAUDE.md §4.4 — masking is
/// a logging concern; storage and IPC return the full value to the user who
/// already owns the data). `finding_id` is the SHA-256 of
/// `aws_account_id:rule_key` and is stable across scans.
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub finding_id: String,
    pub aws_account_id: String,
    pub rule_key: String,
    /// The raw, unmapped finding type as emitted by the scanner. Preserved
    /// so the UI can show unrecognized findings under an "Other" group
    /// without losing the original identifier (Contract 07 §Edge Cases).
    pub raw_type: String,
    pub service: String,
    pub severity: Severity,
    pub description: String,
    pub rationale: Option<String>,
    pub dashboard_name: Option<String>,
    /// ScoutSuite's resource-path pattern (e.g. `iam.users.id`). Kept as a
    /// string because pattern grammar is provider-specific.
    pub resource_path_pattern: Option<String>,
    pub checked_items: i64,
    pub flagged_items: i64,
    pub status: FindingStatus,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub first_seen_scan_id: String,
    pub last_seen_scan_id: String,
    pub resolved_at: Option<DateTime<Utc>>,
    pub resolved_in_scan_id: Option<String>,
}

/// A flagged resource that triggered a finding.
///
/// `resource_path` is the scanner's path expression (e.g.
/// `iam.users.id.alice.MfaActive`). If the scanner emitted a malformed ARN
/// or otherwise unparseable identifier, the row is still stored — `invalid`
/// is set to `true` so the UI can surface "unparsed resource" without
/// losing data (Contract 07 §Edge Cases).
#[derive(Debug, Clone, Serialize)]
pub struct FindingResource {
    pub finding_id: String,
    pub aws_account_id: String,
    pub resource_path: String,
    pub invalid: bool,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    // PR #82 — identity fields walked out of the ScoutSuite output's
    // deepest resource-entity ancestor. All optional; `None` means
    // either the path didn't land on a dict with identifying scalars
    // (e.g. `iam.password_policy.*` globals) or the row predates the
    // 0014 migration that added the columns.
    pub resource_name: Option<String>,
    pub resource_arn: Option<String>,
    pub resource_id_value: Option<String>,
    /// Forward-compat attribute bag. Map of `{key: scalar JSON}` —
    /// strings, numbers, and booleans only. Empty map when the walk
    /// captured nothing (legacy rows, or entities without extra
    /// scalars). The frontend renders every non-null key by default.
    pub attributes: serde_json::Map<String, serde_json::Value>,
}

/// Detail payload returned by `get_finding`. The finding row plus its full
/// resource list. UI dialogs render this in the Findings detail drawer.
#[derive(Debug, Clone, Serialize)]
pub struct FindingDetail {
    pub finding: Finding,
    pub resources: Vec<FindingResource>,
}

/// Optional filter applied to `list_findings(scan_id, filter)`. Every field
/// is optional — `None` means "no constraint". The filter is evaluated in
/// Rust against an index-backed query (Contract 07 §Acceptance Criteria).
///
/// `severity` is a list so the UI can show "Critical OR High" without
/// issuing two separate queries.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct FindingsFilter {
    pub severity: Vec<Severity>,
    pub service: Option<String>,
    pub status: Option<FindingStatus>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// Summary returned by `parse_and_store`. The frontend renders these
/// counters in the post-scan toast so the user knows whether a scan
/// surfaced new findings vs. confirmed prior ones.
#[derive(Debug, Clone, Serialize)]
pub struct ParseSummary {
    pub scan_id: String,
    pub aws_account_id: String,
    pub findings_total: usize,
    pub findings_inserted: usize,
    pub findings_updated: usize,
    pub findings_resolved: usize,
    pub resources_inserted: usize,
    pub resources_updated: usize,
    /// Findings whose raw level was unrecognized and which were mapped to
    /// `Informational`. Surfaced so QA can confirm the mapping happened
    /// without grepping logs.
    pub unknown_severity_count: usize,
    /// Findings whose `raw_type` was not in CloudSaw's recognized catalog.
    /// Stored anyway with `raw_type` preserved; this counter lets the UI
    /// flag "N unrecognized finding types" so users can ask us to add
    /// them.
    pub unknown_type_count: usize,
}

/// Result of `delete_scan`. Reports how many rows the cascade removed so
/// the UI can confirm to the user.
#[derive(Debug, Clone, Serialize)]
pub struct DeleteScanImpact {
    pub scan_id: String,
    pub findings_removed: usize,
    pub resources_removed: usize,
    pub findings_updated: usize,
}
