-- Migration 0003 — multi-account configuration (Contract 04).
--
-- Stores one row per CloudSaw "account": a user-supplied label paired with the
-- AWS profile that resolves to it, plus the verified AWS account ID and
-- provisioning/scan status. CREDENTIALS ARE NEVER STORED HERE (CLAUDE.md §4.3
-- and Contract 04 §Constraints). Only configuration: labels, profile names,
-- account IDs, environment tags, status timestamps.
--
-- Design notes:
--   * `aws_account_id` is the primary key AND the partitioning key used by
--     every later account-scoped table (findings, scans, events, ...). It is
--     the 12-digit AWS account ID, verified live against sts:GetCallerIdentity
--     before the row is inserted.
--   * `label` is UNIQUE so a user can't create ambiguous "dev" duplicates.
--   * `environment` is a free-text tag validated in Rust to one of
--     {dev, staging, prod, other}; storing TEXT keeps later vocabulary growth
--     a no-migration change.
--   * `active_account` is a singleton table (id=1 invariant, same pattern as
--     `app_lock`). Removing the active account clears the column rather than
--     relying on SQLite FK cascades, since foreign_keys is off by default and
--     CloudSaw doesn't toggle it (it would surprise other queries).

CREATE TABLE accounts (
    aws_account_id          TEXT PRIMARY KEY,
    label                   TEXT NOT NULL,
    profile_name            TEXT NOT NULL,
    environment             TEXT NOT NULL,
    role_provisioned        INTEGER NOT NULL DEFAULT 0,
    role_provisioned_at     TEXT,
    last_scan_at            TEXT,
    last_scan_status        TEXT,
    created_at              TEXT NOT NULL,
    updated_at              TEXT NOT NULL
);

CREATE UNIQUE INDEX accounts_label_unique ON accounts(label);

CREATE TABLE active_account (
    id              INTEGER PRIMARY KEY CHECK (id = 1),
    aws_account_id  TEXT,
    updated_at      TEXT NOT NULL
);

INSERT INTO active_account (id, aws_account_id, updated_at)
VALUES (1, NULL, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));

-- Display preference: by default, account IDs are masked in the UI to the last
-- 4 digits (Contract 04 §Constraints). Users can flip this in Settings; logs
-- mask regardless of this value.
INSERT INTO settings (key, value, updated_at)
VALUES ('accounts.reveal_full_ids', '0', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
