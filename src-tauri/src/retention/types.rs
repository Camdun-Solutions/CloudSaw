// Public types for the retention engine.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// One retention policy. `Never` encodes "do not auto-purge" — Contract 11
/// §Edge Cases requires the UI surface this option and the engine honor it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "days")]
pub enum RetentionPeriod {
    Days(u32),
    Never,
}

impl RetentionPeriod {
    pub fn to_storage(self) -> String {
        match self {
            RetentionPeriod::Days(n) => n.to_string(),
            RetentionPeriod::Never => "never".to_string(),
        }
    }

    pub fn from_storage(raw: &str) -> Self {
        let trimmed = raw.trim();
        if trimmed.eq_ignore_ascii_case("never") {
            return RetentionPeriod::Never;
        }
        match trimmed.parse::<u32>() {
            Ok(n) => RetentionPeriod::Days(n),
            // A corrupted value falls back to 90 days — the project-wide
            // default. Better to over-purge than silently never purge.
            Err(_) => RetentionPeriod::Days(90),
        }
    }
}

/// What the Settings UI reads and writes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionSettings {
    pub scan_retention: RetentionPeriod,
    pub eventlog_retention: RetentionPeriod,
    /// RFC3339 of the last successful sweep; `None` if it has never run.
    pub last_run_at: Option<DateTime<Utc>>,
}

/// What `run_now` returns. Surfaced via IPC so the Settings UI can show a
/// "purged N scan dirs and M event log entries" toast.
#[derive(Debug, Clone, Serialize)]
pub struct RetentionRunSummary {
    pub scan_dirs_removed: usize,
    pub raw_files_removed: usize,
    pub eventlog_rows_removed: usize,
    pub scan_cutoff: Option<DateTime<Utc>>,
    pub eventlog_cutoff: Option<DateTime<Utc>>,
}
