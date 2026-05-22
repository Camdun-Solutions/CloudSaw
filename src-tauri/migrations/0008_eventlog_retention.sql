-- Migration 0008 — Event Log, Retention, Deletion & Panic (Contract 11).
--
-- This migration adds two tables and three setting keys:
--
--   * `event_log` — append-only record of user-visible actions. Rows are
--     INSERT-ONLY from application code; UI mutations are blocked by the
--     absence of any update/delete code path (Contract 11 §Constraints).
--     Rows do age out via the retention engine (separate, configurable),
--     but never via a "user deleted this entry" path.
--   * `event_log_view` — a single-row table holding the "cleared at"
--     timestamp the UI honors for the Activity Log view. Clearing the view
--     never deletes underlying rows — it just bumps this marker so the
--     default list query hides earlier entries. Exports still see them.
--
-- Setting keys (in the existing `settings` table from migration 0001):
--   * `retention_scan_days` — raw-scan-output retention period in days.
--     `NULL` means "never auto-purge" (Contract 11 §Edge Cases).
--   * `retention_eventlog_days` — event-log retention period in days.
--     `NULL` means "never auto-purge".
--   * `retention_last_run_at` — RFC3339 timestamp of the last retention
--     sweep. Purely informational; the engine reruns on bootstrap and on
--     demand so a stale value is never load-bearing.
--
-- Defaults (Contract 11 §Expected Output): both retention periods default
-- to 90 days. We insert the defaults here so the Settings UI reads a
-- value rather than racing the first write.
--
-- Findings metadata is INTENTIONALLY not subject to a retention sweep
-- (Contract 11 §Constraints + Acceptance Criteria). The dashboard's
-- trend/drift views depend on the full history, and retention only
-- targets raw scan output (on disk) and event-log rows (in this table).

CREATE TABLE event_log (
    event_id       TEXT PRIMARY KEY,
    occurred_at    TEXT NOT NULL,
    kind           TEXT NOT NULL,
    summary        TEXT NOT NULL,
    detail         TEXT,
    aws_account_id TEXT,
    scan_id        TEXT,
    path           TEXT,
    item_count     INTEGER
);

-- Most reads are "newest first, optionally filtered by kind". Two indexes
-- cover the two query shapes.
CREATE INDEX event_log_occurred_at ON event_log(occurred_at DESC);
CREATE INDEX event_log_kind_occurred_at ON event_log(kind, occurred_at DESC);

CREATE TABLE event_log_view (
    id           INTEGER PRIMARY KEY CHECK (id = 1),
    cleared_at   TEXT
);
INSERT INTO event_log_view (id, cleared_at) VALUES (1, NULL);

-- Retention defaults. We write `90` (text) for both because the existing
-- settings.value column is TEXT and the readers parse it as i64 or treat
-- the literal string `never` as "do not auto-purge".
INSERT INTO settings (key, value, updated_at)
VALUES (
    'retention_scan_days',
    '90',
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
);
INSERT INTO settings (key, value, updated_at)
VALUES (
    'retention_eventlog_days',
    '90',
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
);
INSERT INTO settings (key, value, updated_at)
VALUES (
    'retention_last_run_at',
    '',
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
);
