// Public data types for the scheduler (Contract 10).
//
// All types are plain serializable structs — no AWS SDK types, no credential
// material, no SDK errors cross IPC. The cadence enum mirrors what the
// SQLite `schedules` table can describe; serialization uses the same
// `tag/content` shape the lock module uses for `LockPeriod`, so the
// frontend can round-trip cadences without an adapter.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// How often a scheduled scan should run.
///
/// `Daily`, `Weekly`, and `Monthly` are anchored to a time-of-day; `Interval`
/// fires every N minutes regardless of clock time. Each variant carries the
/// data needed to recompute `next_run_at` without consulting the original
/// row's history — that lets the runner be stateless across restarts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ScheduleCadence {
    /// Every day at `time_of_day_minutes` (local time, 0..=1439).
    Daily,
    /// Every week on `day_of_week` (0 = Sunday, 6 = Saturday) at
    /// `time_of_day_minutes`.
    Weekly { day_of_week: u8 },
    /// Every month on `day_of_month` (1..=28 — clamped to 28 so the day
    /// always exists, regardless of month length) at `time_of_day_minutes`.
    Monthly { day_of_month: u8 },
    /// Every `minutes` minutes (1..=43200, i.e. up to 30 days).
    Interval { minutes: u32 },
}

impl ScheduleCadence {
    pub const MIN_INTERVAL_MINUTES: u32 = 1;
    pub const MAX_INTERVAL_MINUTES: u32 = 43_200;

    pub fn kind_str(&self) -> &'static str {
        match self {
            ScheduleCadence::Daily => "daily",
            ScheduleCadence::Weekly { .. } => "weekly",
            ScheduleCadence::Monthly { .. } => "monthly",
            ScheduleCadence::Interval { .. } => "interval",
        }
    }

    /// Reconstruct from the storage shape (`kind`, `value`). Returns `None`
    /// when either field falls outside the documented bounds — the caller
    /// surfaces a typed error.
    pub fn from_storage(kind: &str, value: i64) -> Option<ScheduleCadence> {
        match kind {
            "daily" => Some(ScheduleCadence::Daily),
            "weekly" => {
                let dow = u8::try_from(value).ok()?;
                if dow <= 6 {
                    Some(ScheduleCadence::Weekly { day_of_week: dow })
                } else {
                    None
                }
            }
            "monthly" => {
                let dom = u8::try_from(value).ok()?;
                if (1..=28).contains(&dom) {
                    Some(ScheduleCadence::Monthly { day_of_month: dom })
                } else {
                    None
                }
            }
            "interval" => {
                let mins = u32::try_from(value).ok()?;
                if (Self::MIN_INTERVAL_MINUTES..=Self::MAX_INTERVAL_MINUTES).contains(&mins) {
                    Some(ScheduleCadence::Interval { minutes: mins })
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    pub fn cadence_value(&self) -> i64 {
        match self {
            ScheduleCadence::Daily => 0,
            ScheduleCadence::Weekly { day_of_week } => *day_of_week as i64,
            ScheduleCadence::Monthly { day_of_month } => *day_of_month as i64,
            ScheduleCadence::Interval { minutes } => *minutes as i64,
        }
    }
}

/// The most recent thing the scheduler did for an account. Surfaced to the
/// UI so the Settings panel can display "fired at 09:00", "skipped — already
/// running", etc. without re-deriving from the event log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LastRunOutcome {
    /// A scan was started by the scheduler.
    Fired,
    /// Time arrived but a scan for the account was already in flight, so the
    /// schedule was skipped to honor the one-scan-per-account rule.
    SkippedAlreadyRunning,
    /// Time arrived but the scanner role was not provisioned for this
    /// account, so the schedule was skipped with a clear reason.
    SkippedRoleNotProvisioned,
    /// Time arrived but the bundled ScoutSuite binary failed its integrity
    /// check — the schedule refuses to spawn a tampered scanner.
    SkippedScannerUnavailable,
    /// Catch-up run on next launch, after one or more scheduled times were
    /// missed while the app was closed. Only ONE catch-up fires regardless
    /// of how many slots were missed.
    CatchUp,
    /// Internal error (db write failure, sandbox mismatch). Stable tag; raw
    /// errors never reach this surface.
    SkippedInternalError,
}

impl LastRunOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            LastRunOutcome::Fired => "fired",
            LastRunOutcome::SkippedAlreadyRunning => "skipped_already_running",
            LastRunOutcome::SkippedRoleNotProvisioned => "skipped_role_not_provisioned",
            LastRunOutcome::SkippedScannerUnavailable => "skipped_scanner_unavailable",
            LastRunOutcome::CatchUp => "catch_up",
            LastRunOutcome::SkippedInternalError => "skipped_internal_error",
        }
    }

    pub fn from_storage(s: &str) -> Option<LastRunOutcome> {
        match s {
            "fired" => Some(LastRunOutcome::Fired),
            "skipped_already_running" => Some(LastRunOutcome::SkippedAlreadyRunning),
            "skipped_role_not_provisioned" => Some(LastRunOutcome::SkippedRoleNotProvisioned),
            "skipped_scanner_unavailable" => Some(LastRunOutcome::SkippedScannerUnavailable),
            "catch_up" => Some(LastRunOutcome::CatchUp),
            "skipped_internal_error" => Some(LastRunOutcome::SkippedInternalError),
            _ => None,
        }
    }
}

/// One schedule row ready to render in the UI. The `next_run_at` field is the
/// precomputed RFC3339 timestamp the background runner compares against.
#[derive(Debug, Clone, Serialize)]
pub struct Schedule {
    pub aws_account_id: String,
    pub cadence: ScheduleCadence,
    pub time_of_day_minutes: Option<u16>,
    pub enabled: bool,
    pub last_run_at: Option<DateTime<Utc>>,
    pub last_run_outcome: Option<LastRunOutcome>,
    pub last_run_scan_id: Option<String>,
    pub next_run_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// IPC input: payload of `scheduler_set_schedule`. The frontend supplies the
/// cadence, time-of-day, and enabled flag — the backend validates and writes.
#[derive(Debug, Clone, Deserialize)]
pub struct SetScheduleInput {
    pub aws_account_id: String,
    pub cadence: ScheduleCadence,
    /// Time-of-day in minutes from local midnight (0..=1439). Required for
    /// daily/weekly/monthly cadences; ignored for interval.
    pub time_of_day_minutes: Option<u16>,
    pub enabled: bool,
}

/// The kind of event recorded in `schedule_events`. Stable tags — the UI
/// localizes the message. Configuration changes (`set`, `clear`,
/// `enable`, `disable`) are recorded alongside firing decisions so the
/// event log shows the full lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleEventKind {
    /// `set_schedule` was called — created a new schedule or replaced an
    /// existing one's configuration.
    ConfigSet,
    /// `clear_schedule` was called.
    ConfigCleared,
    /// `enabled` was flipped from false to true via `set_schedule`.
    Enabled,
    /// `enabled` was flipped from true to false via `set_schedule`.
    Disabled,
    /// The background runner triggered a scan for this schedule.
    Fired,
    /// The background runner skipped a scheduled time. `reason` carries
    /// the stable tag (matches `LastRunOutcome` for skips).
    Skipped,
    /// The background runner triggered a single catch-up scan after the app
    /// resumed across one or more missed scheduled times.
    CatchUp,
}

impl ScheduleEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ScheduleEventKind::ConfigSet => "config_set",
            ScheduleEventKind::ConfigCleared => "config_cleared",
            ScheduleEventKind::Enabled => "enabled",
            ScheduleEventKind::Disabled => "disabled",
            ScheduleEventKind::Fired => "fired",
            ScheduleEventKind::Skipped => "skipped",
            ScheduleEventKind::CatchUp => "catch_up",
        }
    }

    pub fn from_storage(s: &str) -> Option<ScheduleEventKind> {
        match s {
            "config_set" => Some(ScheduleEventKind::ConfigSet),
            "config_cleared" => Some(ScheduleEventKind::ConfigCleared),
            "enabled" => Some(ScheduleEventKind::Enabled),
            "disabled" => Some(ScheduleEventKind::Disabled),
            "fired" => Some(ScheduleEventKind::Fired),
            "skipped" => Some(ScheduleEventKind::Skipped),
            "catch_up" => Some(ScheduleEventKind::CatchUp),
            _ => None,
        }
    }
}

/// One row from the `schedule_events` table.
#[derive(Debug, Clone, Serialize)]
pub struct ScheduleEvent {
    pub event_id: String,
    pub aws_account_id: String,
    pub occurred_at: DateTime<Utc>,
    pub kind: ScheduleEventKind,
    pub reason: Option<String>,
    pub scan_id: Option<String>,
}

/// IPC return: the next-run timestamps for the schedules the caller asked
/// about. The map is keyed by `aws_account_id`. Accounts without a schedule
/// (or with a disabled one) are returned with `None`.
#[derive(Debug, Clone, Serialize)]
pub struct NextRunTime {
    pub aws_account_id: String,
    pub next_run_at: Option<DateTime<Utc>>,
}
