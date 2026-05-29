// Public types for the event log. Every value here is safe to render in the
// UI — no credentials, tokens, or secret values ever land in an event-log
// row (Contract 11 §Constraints).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Stable, enumerated event categories. Adding a new variant requires a
/// frontend translation key under `eventlog.kind.<variant>` so the activity
/// log can localize the label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    AppStarted,
    AppStopping,
    ScanCompleted,
    ScanFailed,
    ScanCanceled,
    ScheduledScanFired,
    ScheduledScanSkipped,
    GithubTicketCreated,
    MasterPasswordChanged,
    MasterPasswordReset,
    AccountAdded,
    AccountRemoved,
    ScanDeleted,
    Export,
    PanicWipe,
    SettingsChanged,
    RetentionPurged,
    /// PR #70 — emitted by `findings::storage::apply_parsed` when a
    /// scan auto-resolves one or more prior findings (the resolution
    /// sweep). The `item_count` field carries the resolved count.
    FindingsAutoResolved,
}

impl EventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EventKind::AppStarted => "app_started",
            EventKind::AppStopping => "app_stopping",
            EventKind::ScanCompleted => "scan_completed",
            EventKind::ScanFailed => "scan_failed",
            EventKind::ScanCanceled => "scan_canceled",
            EventKind::ScheduledScanFired => "scheduled_scan_fired",
            EventKind::ScheduledScanSkipped => "scheduled_scan_skipped",
            EventKind::GithubTicketCreated => "github_ticket_created",
            EventKind::MasterPasswordChanged => "master_password_changed",
            EventKind::MasterPasswordReset => "master_password_reset",
            EventKind::AccountAdded => "account_added",
            EventKind::AccountRemoved => "account_removed",
            EventKind::ScanDeleted => "scan_deleted",
            EventKind::Export => "export",
            EventKind::PanicWipe => "panic_wipe",
            EventKind::SettingsChanged => "settings_changed",
            EventKind::RetentionPurged => "retention_purged",
            EventKind::FindingsAutoResolved => "findings_auto_resolved",
        }
    }

    pub fn from_storage(s: &str) -> Option<Self> {
        Some(match s {
            "app_started" => EventKind::AppStarted,
            "app_stopping" => EventKind::AppStopping,
            "scan_completed" => EventKind::ScanCompleted,
            "scan_failed" => EventKind::ScanFailed,
            "scan_canceled" => EventKind::ScanCanceled,
            "scheduled_scan_fired" => EventKind::ScheduledScanFired,
            "scheduled_scan_skipped" => EventKind::ScheduledScanSkipped,
            "github_ticket_created" => EventKind::GithubTicketCreated,
            "master_password_changed" => EventKind::MasterPasswordChanged,
            "master_password_reset" => EventKind::MasterPasswordReset,
            "account_added" => EventKind::AccountAdded,
            "account_removed" => EventKind::AccountRemoved,
            "scan_deleted" => EventKind::ScanDeleted,
            "export" => EventKind::Export,
            "panic_wipe" => EventKind::PanicWipe,
            "settings_changed" => EventKind::SettingsChanged,
            "retention_purged" => EventKind::RetentionPurged,
            "findings_auto_resolved" => EventKind::FindingsAutoResolved,
            _ => return None,
        })
    }
}

/// What `record_event` accepts. Constructed with the small public builder
/// so callers in other modules don't need to learn the full row shape.
#[derive(Debug, Clone)]
pub struct EventInput {
    pub kind: EventKind,
    pub summary: String,
    pub detail: Option<String>,
    pub aws_account_id: Option<String>,
    pub scan_id: Option<String>,
    pub path: Option<String>,
    pub item_count: Option<i64>,
}

impl EventInput {
    /// Minimal builder for the common case: kind + summary, everything else
    /// defaulted to None. Other fields are populated via the `with_*` chain.
    pub fn new(kind: EventKind, summary: impl Into<String>) -> Self {
        Self {
            kind,
            summary: summary.into(),
            detail: None,
            aws_account_id: None,
            scan_id: None,
            path: None,
            item_count: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn with_account(mut self, aws_account_id: impl Into<String>) -> Self {
        self.aws_account_id = Some(aws_account_id.into());
        self
    }

    pub fn with_scan_id(mut self, scan_id: impl Into<String>) -> Self {
        self.scan_id = Some(scan_id.into());
        self
    }

    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn with_item_count(mut self, item_count: i64) -> Self {
        self.item_count = Some(item_count);
        self
    }
}

/// One row of the event_log table as it crosses the IPC boundary. The
/// `aws_account_id` is masked to the last 4 digits here — the full ID
/// never crosses IPC in event-log payloads (CLAUDE.md §4.4 redaction).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventLogEntry {
    pub event_id: String,
    pub occurred_at: DateTime<Utc>,
    pub kind: EventKind,
    pub summary: String,
    pub detail: Option<String>,
    /// Masked to `****dddd` form for the UI. Full ID is never returned.
    pub aws_account_id_masked: Option<String>,
    pub scan_id: Option<String>,
    pub path: Option<String>,
    pub item_count: Option<i64>,
}

/// Filter for `list_events`. Empty fields mean "no filter on this axis".
#[derive(Debug, Clone, Default, Deserialize)]
pub struct EventLogFilter {
    /// One or more kinds to include. Empty == all kinds.
    #[serde(default)]
    pub kinds: Vec<EventKind>,
    /// Earliest occurred_at to include (inclusive).
    #[serde(default)]
    pub since: Option<DateTime<Utc>>,
    /// Latest occurred_at to include (inclusive).
    #[serde(default)]
    pub until: Option<DateTime<Utc>>,
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
    /// When true, ignore the cleared-view marker and return every row
    /// (used by the Export action). When false (default), entries strictly
    /// before `event_log_view.cleared_at` are hidden.
    #[serde(default)]
    pub include_cleared: bool,
}
