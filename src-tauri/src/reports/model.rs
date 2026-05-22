// Data model for both report shapes (per-scan and custom). The
// aggregator produces a `ReportContent`; `html` and `pdf` render the
// same value into their respective output formats. Pure values — no
// IO, no IPC. The contract's "what you preview is what gets exported"
// invariant follows from the fact that BOTH renderers consume the
// same struct.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::findings::{FindingStatus, Severity};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportKind {
    PerScan,
    Custom,
}

/// What ID-disclosure mode the report renders under. Mirrors the
/// project-wide masking rule (last 4 digits) for `Masked`, and emits
/// the full 12 digits only on `Full`. Contract 15 §Constraints +
/// §Acceptance Criteria require the user to explicitly opt in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountIdDisclosure {
    Masked,
    Full,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReportHeader {
    pub kind: ReportKind,
    pub title: String,
    pub subtitle: Option<String>,
    pub generated_at: DateTime<Utc>,
    pub cloudsaw_version: String,
    /// Banner shown at the top of every report reminding the user to
    /// review for sensitive data before sharing.
    pub review_banner: String,
    pub disclosure: AccountIdDisclosure,
    pub locale: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SeverityCounts {
    pub critical: usize,
    pub high: usize,
    pub medium: usize,
    pub low: usize,
    pub informational: usize,
}

impl SeverityCounts {
    pub fn total(&self) -> usize {
        self.critical + self.high + self.medium + self.low + self.informational
    }

    pub fn empty() -> Self {
        SeverityCounts {
            critical: 0,
            high: 0,
            medium: 0,
            low: 0,
            informational: 0,
        }
    }

    pub fn bump(&mut self, severity: Severity) {
        match severity {
            Severity::Critical => self.critical += 1,
            Severity::High => self.high += 1,
            Severity::Medium => self.medium += 1,
            Severity::Low => self.low += 1,
            Severity::Informational => self.informational += 1,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountRef {
    /// Already-rendered display string per the chosen disclosure mode.
    /// The renderer NEVER re-applies masking; it writes this value
    /// verbatim into the output.
    pub display: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanSummary {
    pub scan_id: String,
    pub account: AccountRef,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub status: String,
    pub severity_counts: SeverityCounts,
}

#[derive(Debug, Clone, Serialize)]
pub struct FindingRow {
    pub finding_id: String,
    pub rule_key: String,
    pub service: String,
    pub severity: Severity,
    pub status: FindingStatus,
    pub description: String,
    pub rationale: Option<String>,
    pub checked_items: i64,
    pub flagged_items: i64,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    /// KB article body, already redacted. Empty when no matched article.
    pub remediation: String,
    /// Compliance control mapping summary lines, one per framework.
    pub compliance_lines: Vec<String>,
    /// Resource paths the finding touches. Already masked to the
    /// disclosure mode the caller picked — the renderer writes
    /// verbatim. Capped at 50 entries with a "+N more" suffix in
    /// `truncated_extra`.
    pub resources: Vec<String>,
    pub truncated_extra: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct PerServiceTotals {
    pub service: String,
    pub findings: usize,
    pub severity_counts: SeverityCounts,
}

#[derive(Debug, Clone, Serialize)]
pub struct EventRow {
    pub occurred_at: DateTime<Utc>,
    pub kind: String,
    pub summary: String,
    pub account_display: Option<String>,
    pub scan_id: Option<String>,
}

/// Top-level report payload. Both per-scan and custom reports use this
/// shape; the renderers branch only on which sections to include.
#[derive(Debug, Clone, Serialize)]
pub struct ReportContent {
    pub header: ReportHeader,
    /// One entry for a per-scan report; one per scan that touches the
    /// custom range otherwise. Newest first.
    pub scans: Vec<ScanSummary>,
    /// All findings the report enumerates, in severity-then-last-seen order.
    pub findings: Vec<FindingRow>,
    /// Per-service rollup. Empty when there are zero findings.
    pub per_service: Vec<PerServiceTotals>,
    /// Events relevant to the report (scan completions, exports,
    /// retention purges). Empty for the per-scan kind.
    pub events: Vec<EventRow>,
    /// Optional explicit empty-state copy the renderer surfaces when
    /// `findings.is_empty()`.
    pub empty_state_note: Option<String>,
}

/// Result returned to the UI from a successful export. The IPC bridge
/// surfaces this so the UI can show "saved to <path>" + the
/// auto-export status as a separate row.
#[derive(Debug, Clone, Serialize)]
pub struct ExportOutcome {
    pub primary_path: String,
    pub bytes_written: u64,
    pub auto_export_path: Option<String>,
    pub auto_export_failed: bool,
}
