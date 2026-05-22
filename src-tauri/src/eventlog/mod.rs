// Event log — Contract 11A.
//
// Append-only record of user-visible actions: scan completions, GitHub
// ticket creations, master-password changes, deletions, exports, panic,
// and app start/stop. Contract 11 §Constraints:
//
//   * The log is append-only and not editable from the UI. "Clear all" only
//     clears the *view* — underlying rows persist subject to the
//     event-log retention policy (Contract 11B) and still appear in Export.
//   * No secret values land in event-log rows. Deletions are recorded as a
//     count + affected paths, never as content. Account IDs are masked to
//     the last 4 digits before they cross IPC (see `EventLogEntry`).
//   * The retention engine is the only path that DELETEs rows, and the
//     panic wipe is the only path that hard-clears the whole table.
//
// Public surface (mirrors the contract's "Expected Output"):
//
//     record_event(input)            -> ()
//     list_events(filter)            -> Vec<EventLogEntry>
//     search_events(query, limit)    -> Vec<EventLogEntry>
//     export_events()                -> String  (newline-delimited JSON)
//     clear_event_view()             -> ()

pub mod error;
pub mod storage;
pub mod types;

pub use error::EventLogError;
pub use types::{EventInput, EventKind, EventLogEntry, EventLogFilter};

use chrono::Utc;
use rand_core::{OsRng, RngCore};

/// Maximum length of a single summary or detail field. Long inputs are
/// truncated rather than rejected — the event log must never block a real
/// action just because a caller passed a verbose message.
const SUMMARY_CAP: usize = 280;
const DETAIL_CAP: usize = 2_000;

/// Mint a fresh event ID. 128 random bits, hex-encoded — same shape as
/// scheduler::runner::mint_event_id.
pub fn mint_event_id() -> String {
    let mut bytes = [0u8; 16];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Record one event. Best-effort: this function never returns an error
/// to its caller, because callers (scanner, accounts, applock, etc.)
/// should not have their primary action fail because the event log was
/// temporarily unavailable. The internal write errors are swallowed and
/// surfaced via tracing in the future; today they're silent — same
/// fail-soft pattern the scheduler module uses for its own event log.
pub fn record_event(input: EventInput) {
    let _ = record_event_strict(input);
}

/// Strict variant used by tests and the IPC bridge so the QA contract can
/// assert "this call wrote a row." Validates inputs and returns the typed
/// error on failure.
pub fn record_event_strict(input: EventInput) -> Result<String, EventLogError> {
    let mut input = input;
    input.summary = truncate(&input.summary, SUMMARY_CAP);
    input.detail = input.detail.map(|d| truncate(&d, DETAIL_CAP));

    if let Some(id) = &input.aws_account_id {
        if !is_valid_account_id(id) {
            return Err(EventLogError::InvalidInput("aws_account_id"));
        }
    }
    if let Some(scan_id) = &input.scan_id {
        if scan_id.is_empty() || scan_id.len() > 128 {
            return Err(EventLogError::InvalidInput("scan_id"));
        }
        if !scan_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            return Err(EventLogError::InvalidInput("scan_id"));
        }
    }

    let event_id = mint_event_id();
    let occurred_at = Utc::now();
    storage::insert(&event_id, occurred_at, &input)?;
    Ok(event_id)
}

/// Convenience for "I have a kind and a summary." Used by the bulk of
/// emitters that don't need the full builder.
pub fn record_simple(kind: EventKind, summary: impl Into<String>) {
    record_event(EventInput::new(kind, summary));
}

/// Newest-first paginated list, honoring the cleared-view marker by
/// default. Pass `filter.include_cleared = true` to bypass it.
pub fn list_events(filter: EventLogFilter) -> Result<Vec<EventLogEntry>, EventLogError> {
    storage::list(&filter)
}

/// Substring search across the `summary` and `detail` fields. Limited to
/// the most recent matches so a runaway query can't drag the UI down.
pub fn search_events(query: &str, limit: Option<i64>) -> Result<Vec<EventLogEntry>, EventLogError> {
    if query.is_empty() {
        return list_events(EventLogFilter {
            limit,
            ..Default::default()
        });
    }
    if query.len() > 256 {
        return Err(EventLogError::InvalidInput("query"));
    }
    storage::search(query, limit.unwrap_or(500))
}

/// Render the full log (bypassing the cleared marker) as newline-delimited
/// JSON. Each line is one event object; the format is intentionally
/// machine-parseable so users can grep, diff, or import elsewhere.
pub fn export_events() -> Result<String, EventLogError> {
    let entries = storage::list_all_for_export()?;
    let mut out = String::with_capacity(entries.len() * 200);
    for entry in entries {
        let line = serde_json::to_string(&entry)
            .map_err(|e| EventLogError::Db(format!("serialize event: {e}")))?;
        out.push_str(&line);
        out.push('\n');
    }
    Ok(out)
}

/// Set the "cleared at" marker so the default list query hides earlier
/// entries. Underlying rows are not deleted (Contract 11 §Constraints).
/// Records its own event so the user can see the clear happened.
pub fn clear_event_view() -> Result<(), EventLogError> {
    let now = Utc::now();
    // Record the clear FIRST, so a user who immediately re-opens the
    // activity log sees the entry. The clear marker is set after, so the
    // "clear" entry itself stays visible.
    let _ = record_event_strict(
        EventInput::new(EventKind::SettingsChanged, "Activity log view cleared.")
            .with_detail("cleared_via_ui"),
    );
    storage::set_cleared_at(now)
}

/// Read the cleared-view marker. Used by the UI to render a "cleared at X"
/// hint above the list.
pub fn get_cleared_at() -> Result<Option<chrono::DateTime<Utc>>, EventLogError> {
    storage::get_cleared_at()
}

/// Total number of stored events. Used by the Settings UI as a count
/// indicator and by the QA contract.
pub fn count_events() -> Result<i64, EventLogError> {
    storage::count()
}

fn truncate(s: &str, cap: usize) -> String {
    if s.chars().count() <= cap {
        return s.to_string();
    }
    let mut out: String = s.chars().take(cap.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn is_valid_account_id(id: &str) -> bool {
    id.len() == 12 && id.chars().all(|c| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_caps_long_input_with_ellipsis() {
        let s = "x".repeat(SUMMARY_CAP + 50);
        let truncated = truncate(&s, SUMMARY_CAP);
        assert_eq!(truncated.chars().count(), SUMMARY_CAP);
        assert!(truncated.ends_with('…'));
    }

    #[test]
    fn truncate_leaves_short_input_alone() {
        assert_eq!(truncate("hi", SUMMARY_CAP), "hi");
    }

    #[test]
    fn account_id_validation_rejects_garbage() {
        assert!(is_valid_account_id("111122223333"));
        assert!(!is_valid_account_id("11112222333"));
        assert!(!is_valid_account_id("11112222333a"));
        assert!(!is_valid_account_id(""));
    }
}
