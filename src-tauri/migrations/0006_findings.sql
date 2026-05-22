-- Migration 0006 — Findings parser & store (Contract 07).
--
-- Stores the normalized output of ScoutSuite scans. Three tables:
--
--   findings           — one row per (account, rule_key). Aggregated state
--                        across scans. Holds the latest description, severity,
--                        flag counts, and the time-series bookkeeping
--                        (first_seen_at, last_seen_at, resolved_at).
--
--   scan_findings      — many-to-many association recording which findings
--                        were observed in which scans. Drives both the
--                        `list_findings(scan_id)` query and the cascade logic
--                        of `delete_scan`.
--
--   finding_resources  — one row per (finding, resource_path). Aggregated
--                        like `findings`: each resource is recorded once,
--                        with its own first/last-seen timestamps. `invalid`
--                        flags rows whose `resource_path` did not parse as a
--                        valid ARN or ScoutSuite path expression — they are
--                        still stored so the UI can show them under an
--                        "unparsed resource" affordance (Contract 07 §Edge
--                        Cases).
--
-- All three tables are partitioned by `aws_account_id` (inherited from
-- migration 0003). The contract requires it on every finding/scan query
-- (CLAUDE.md §4.1 / Contract 07 §Constraints). The duplicated account_id on
-- `finding_resources` is intentional: it lets resource-list queries filter
-- by account without a join through `findings`.
--
-- Severity normalization is enforced in Rust before insert (one of
-- critical|high|medium|low|informational). The CHECK constraint below is a
-- defense-in-depth backstop only.
--
-- Finding identity (`finding_id`) is the SHA-256 of
-- `<aws_account_id>:<rule_key>`, hex-encoded. Stable across scans by
-- construction, so a re-scan updates rather than duplicates the row.
--
-- `last_seen_at` is the partial source-of-truth for idempotency: the parser
-- only writes an UPDATE when the current scan's started_at is greater than
-- or equal to the row's existing `last_seen_at`. Re-parsing the same scan
-- is therefore a no-op (CLAUDE.md §4.5 + Contract 07 §Acceptance Criteria).

CREATE TABLE findings (
    finding_id            TEXT PRIMARY KEY,
    aws_account_id        TEXT NOT NULL,
    rule_key              TEXT NOT NULL,
    raw_type              TEXT NOT NULL,
    service               TEXT NOT NULL,
    severity              TEXT NOT NULL CHECK (severity IN (
                              'critical', 'high', 'medium', 'low', 'informational'
                          )),
    description           TEXT NOT NULL,
    rationale             TEXT,
    dashboard_name        TEXT,
    resource_path_pattern TEXT,
    checked_items         INTEGER NOT NULL DEFAULT 0,
    flagged_items         INTEGER NOT NULL DEFAULT 0,
    status                TEXT NOT NULL CHECK (status IN ('open', 'resolved')),
    first_seen_at         TEXT NOT NULL,
    last_seen_at          TEXT NOT NULL,
    first_seen_scan_id    TEXT NOT NULL,
    last_seen_scan_id     TEXT NOT NULL,
    resolved_at           TEXT,
    resolved_in_scan_id   TEXT,
    UNIQUE (aws_account_id, rule_key)
);

-- Index choices: every list-view query in Contract 07 starts with
-- aws_account_id (the partition key) and then narrows by one of
-- severity / status / last_seen_at. Compound indexes here keep the
-- severity-filtered list query index-backed at 50k+ findings (Contract 07
-- §Responsiveness target). Verified via EXPLAIN QUERY PLAN in the QA tests.
CREATE INDEX findings_account_severity
    ON findings(aws_account_id, severity, last_seen_at DESC);
CREATE INDEX findings_account_status
    ON findings(aws_account_id, status, last_seen_at DESC);
CREATE INDEX findings_account_service
    ON findings(aws_account_id, service);
CREATE INDEX findings_account_last_seen
    ON findings(aws_account_id, last_seen_at DESC);
CREATE INDEX findings_last_seen_scan
    ON findings(last_seen_scan_id);

CREATE TABLE scan_findings (
    scan_id         TEXT NOT NULL,
    finding_id      TEXT NOT NULL,
    aws_account_id  TEXT NOT NULL,
    observed_at     TEXT NOT NULL,
    PRIMARY KEY (scan_id, finding_id)
);

CREATE INDEX scan_findings_finding ON scan_findings(finding_id);
CREATE INDEX scan_findings_account ON scan_findings(aws_account_id);

CREATE TABLE finding_resources (
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,
    finding_id           TEXT NOT NULL,
    aws_account_id       TEXT NOT NULL,
    resource_path        TEXT NOT NULL,
    invalid              INTEGER NOT NULL DEFAULT 0,
    first_seen_at        TEXT NOT NULL,
    last_seen_at         TEXT NOT NULL,
    first_seen_scan_id   TEXT NOT NULL,
    last_seen_scan_id    TEXT NOT NULL,
    UNIQUE (finding_id, resource_path)
);

CREATE INDEX finding_resources_finding ON finding_resources(finding_id);
CREATE INDEX finding_resources_account ON finding_resources(aws_account_id);
