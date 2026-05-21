-- Migration 0005 — Scanner orchestrator (Contract 06).
--
-- Stores one row per CloudSaw scan attempt. A scan is account-scoped (the
-- `aws_account_id` is the partitioning key inherited from migration 0003) and
-- walks the status state machine documented in Contract 06 §Expected Output:
--   pending → assuming_role → scanning → parsing → complete
--                                                 | complete_with_warnings
--                                                 | failed
--                                                 | canceled
--
-- CREDENTIALS ARE NEVER STORED HERE (CLAUDE.md §4.3 and Contract 06
-- §Constraints). Only run metadata: status, timestamps, output paths, and
-- stable error/warning tags. The role-session-name string identifies the scan
-- on the AWS audit trail; it is constructed from `cloudsaw-scan-<short_id>`
-- and contains no credential material.
--
-- The `raw_output_path` column is relative to the app data root and points at
-- the file Contract 07's parser consumes (`scans/<scan-id>/raw-scout.json`).
-- We persist the path rather than the bytes so the parser can stream large
-- ScoutSuite outputs without round-tripping them through SQLite.
--
-- `pid` is informational — it lets the orchestrator notice a scan whose child
-- process disappeared between app restarts. The recovery path is to mark the
-- row `failed` (see Contract 06 §Edge Cases: "machine sleeps mid-scan").
--
-- `failure_code` and `warning_code` are STABLE TAGS (e.g.
-- "scanner_process_lost", "missing_permissions") that the frontend maps to
-- localized copy. Raw stderr is never stored.

CREATE TABLE scans (
    scan_id               TEXT PRIMARY KEY,
    aws_account_id        TEXT NOT NULL,
    status                TEXT NOT NULL,
    started_at            TEXT NOT NULL,
    finished_at           TEXT,
    failure_code          TEXT,
    warning_code          TEXT,
    warning_detail        TEXT,
    raw_output_path       TEXT,
    role_session_name     TEXT NOT NULL,
    pid                   INTEGER,
    truncated             INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX scans_account_started ON scans(aws_account_id, started_at DESC);
CREATE INDEX scans_status ON scans(status);
