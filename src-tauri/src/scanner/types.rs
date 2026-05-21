// Public data types crossing the IPC boundary for the scanner module.
//
// Per CLAUDE.md §4.1, IPC payloads are plain serializable structs — no AWS
// SDK types, no credential-bearing types, no process handles. Every field
// below is a primitive or a deliberately enumerated tag.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Result of `scanner_detect`. Mirrors the shape of `TerraformAvailability`.
/// A missing or integrity-failed binary blocks `run_scan` — the orchestrator
/// refuses to proceed without a verified ScoutSuite binary.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum ScoutSuiteAvailability {
    /// Binary located and SHA-256 matched the build-pinned hash.
    Available {
        /// Hex SHA-256 of the bundled binary. UI displays the first 12 chars.
        sha256: String,
    },
    /// No binary bundled for this target triple. Production builds before
    /// Next Steps C3 wires up the per-target binary stay in this state.
    Missing,
    /// A binary was located but its SHA-256 did not match the build-pinned
    /// hash. Execution is refused until the user reinstalls a known-good
    /// CloudSaw build.
    IntegrityFailed,
}

/// One scan's lifecycle state. See Contract 06 §Expected Output for the
/// transition graph. Terminal states are `Complete`, `CompleteWithWarnings`,
/// `Failed`, and `Canceled`; all others are transient.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanStatus {
    Pending,
    AssumingRole,
    Scanning,
    Parsing,
    Complete,
    CompleteWithWarnings,
    Failed,
    Canceled,
}

impl ScanStatus {
    /// Stable string form persisted in SQLite. Stable across versions.
    pub fn as_str(self) -> &'static str {
        match self {
            ScanStatus::Pending => "pending",
            ScanStatus::AssumingRole => "assuming_role",
            ScanStatus::Scanning => "scanning",
            ScanStatus::Parsing => "parsing",
            ScanStatus::Complete => "complete",
            ScanStatus::CompleteWithWarnings => "complete_with_warnings",
            ScanStatus::Failed => "failed",
            ScanStatus::Canceled => "canceled",
        }
    }

    pub fn from_storage(s: &str) -> Option<ScanStatus> {
        match s {
            "pending" => Some(ScanStatus::Pending),
            "assuming_role" => Some(ScanStatus::AssumingRole),
            "scanning" => Some(ScanStatus::Scanning),
            "parsing" => Some(ScanStatus::Parsing),
            "complete" => Some(ScanStatus::Complete),
            "complete_with_warnings" => Some(ScanStatus::CompleteWithWarnings),
            "failed" => Some(ScanStatus::Failed),
            "canceled" => Some(ScanStatus::Canceled),
            _ => None,
        }
    }

    /// True when the scan can no longer transition. Cancellation and the
    /// "already running" check both consult this.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            ScanStatus::Complete
                | ScanStatus::CompleteWithWarnings
                | ScanStatus::Failed
                | ScanStatus::Canceled
        )
    }
}

/// One scan record, returned by `run_scan`, `scan_status`, and the list APIs.
///
/// `raw_output_path` is the absolute path inside the app data root; it is
/// only populated once the scan reaches `parsing` or a terminal state. The
/// frontend never opens this file directly — Contract 07's parser owns it —
/// but the path is exposed so the UI can offer "Reveal in Finder/Explorer"
/// affordances later.
#[derive(Debug, Clone, Serialize)]
pub struct ScanRecord {
    pub scan_id: String,
    pub aws_account_id: String,
    pub status: ScanStatus,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    /// Stable error tag (e.g. "scanner_process_lost", "assume_role_failed").
    /// `None` unless `status` is `Failed`. Never carries raw stderr.
    pub failure_code: Option<String>,
    /// Stable warning tag (e.g. "missing_permissions"). `None` unless
    /// `status` is `CompleteWithWarnings`.
    pub warning_code: Option<String>,
    /// Optional short detail accompanying the warning. Currently used for
    /// the missing-permission detail Contract 06 §Edge Cases requires; the
    /// payload is a stable tag, never raw text.
    pub warning_detail: Option<String>,
    /// Absolute filesystem path to `raw-scout.json`. `None` until the scan
    /// reaches `parsing` and the output has been persisted.
    pub raw_output_path: Option<String>,
    /// Role-session-name AWS sees on the audit trail. Constructed from
    /// `cloudsaw-scan-<short_id>` so a CloudTrail operator can correlate
    /// CloudTrail entries with CloudSaw scans.
    pub role_session_name: String,
    /// True when the scanner's stdout/stderr was bounded/truncated. The raw
    /// file is still retained; this flag lets the UI surface "output was
    /// large" without exposing the size.
    pub truncated: bool,
}
