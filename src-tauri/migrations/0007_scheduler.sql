-- Migration 0007 — Scheduled & Automated Scans (Contract 10).
--
-- One row per scheduled scan, keyed by `aws_account_id`. A schedule is a
-- non-secret configuration row: cadence, time-of-day, enabled flag, the
-- last run we triggered (or skipped). NO credentials, account ARNs, or
-- scan output land in this table — only the metadata needed to decide
-- "is this account due now?". Credentials still flow through the
-- existing scanner orchestrator (Contract 06), which calls
-- `sts:AssumeRole` fresh per scan (CLAUDE.md §4.3).
--
-- Design notes:
--   * `aws_account_id` is BOTH the primary key AND the partitioning key. One
--     schedule per account; removing the account removes its schedule via the
--     explicit FK below (kept consistent with how `active_account` references
--     `accounts` — we drive the cascade in the `accounts` removal path rather
--     than relying on SQLite FK toggles, which the project keeps off).
--   * `cadence_kind` ∈ {daily, weekly, monthly, interval} matches the Rust
--     `ScheduleCadence` enum (storage stays TEXT so a future cadence is a
--     no-migration change).
--   * `cadence_value` carries the cadence parameter:
--       - daily   → unused (0)
--       - weekly  → day of week, 0 = Sunday, 6 = Saturday
--       - monthly → day of month, 1..28 (capped to 28 so it always exists)
--       - interval→ minutes between runs (1..43200, i.e. up to 30 days)
--   * `time_of_day_minutes` is minutes-from-local-midnight for daily/weekly/
--     monthly cadences (0..1439); `NULL` for interval cadences.
--   * `enabled` toggles the schedule without losing its configuration.
--   * `last_run_at` / `last_run_outcome` / `last_run_scan_id` record what the
--     background runner most recently did — fired a scan, skipped because a
--     scan was already in flight, skipped because the role wasn't provisioned,
--     etc. The event-log integration (Contract 11) reads these alongside the
--     dedicated `schedule_events` rows below.
--   * `next_run_at` is the precomputed RFC3339 timestamp the runner compares
--     against. It is rewritten on every set/fire/skip so the runner never has
--     to recompute cadence math on every poll.
--   * `created_at` / `updated_at` mirror the conventions in `accounts`.
--
-- The companion `schedule_events` table records every fire, skip, and config
-- change with a stable reason tag so the event log (Contract 11) can surface
-- scheduled-scan activity without re-deriving it.

CREATE TABLE schedules (
    aws_account_id          TEXT PRIMARY KEY,
    cadence_kind            TEXT NOT NULL,
    cadence_value           INTEGER NOT NULL DEFAULT 0,
    time_of_day_minutes     INTEGER,
    enabled                 INTEGER NOT NULL DEFAULT 1,
    last_run_at             TEXT,
    last_run_outcome        TEXT,
    last_run_scan_id        TEXT,
    next_run_at             TEXT,
    created_at              TEXT NOT NULL,
    updated_at              TEXT NOT NULL
);

CREATE INDEX schedules_enabled_next_run ON schedules(enabled, next_run_at);

CREATE TABLE schedule_events (
    event_id        TEXT PRIMARY KEY,
    aws_account_id  TEXT NOT NULL,
    occurred_at     TEXT NOT NULL,
    kind            TEXT NOT NULL,
    reason          TEXT,
    scan_id         TEXT
);

CREATE INDEX schedule_events_account_time ON schedule_events(aws_account_id, occurred_at DESC);
