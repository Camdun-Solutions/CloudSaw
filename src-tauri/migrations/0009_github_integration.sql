-- Migration 0009 — GitHub Integration (Contract 12).
--
-- Two new pieces of persistent state, neither of which holds secret
-- material:
--
--   * `finding_tickets` — the local link between a finding (Contract 07)
--     and a GitHub Issue the user filed for it. Stores the public Issue
--     URL, number, and repo coordinates; NEVER stores the PAT, OAuth
--     token, or any credential material.
--   * Two rows in the existing `settings` table (migration 0001) for the
--     user's chosen findings-ticket destination repo. These are TEXT
--     identifiers (`owner/name`) the user types in Settings — they are
--     non-secret. A blank value means "no repo selected; prompt the user
--     before filing a findings ticket."
--
-- The GitHub PAT itself lives ONLY in the OS keychain at
-- `cloudsaw.github_pat` (Contract 12 §Constraints + CLAUDE.md §4.3). It
-- is NOT mirrored here, NOT in any config file, and NOT in any URL.
--
-- One finding can have at most one linked ticket — Contract 12 §Edge Cases:
-- "A finding already has a linked ticket → the UI shows the existing link
-- rather than silently creating a duplicate."

CREATE TABLE finding_tickets (
    finding_id        TEXT PRIMARY KEY,
    aws_account_id    TEXT NOT NULL,
    repo_owner        TEXT NOT NULL,
    repo_name         TEXT NOT NULL,
    issue_number      INTEGER NOT NULL,
    issue_url         TEXT NOT NULL,
    created_at        TEXT NOT NULL
);

CREATE INDEX finding_tickets_account ON finding_tickets(aws_account_id, created_at DESC);

-- Default the findings-ticket target repo to empty so the UI knows to
-- prompt. The error-report repo is hard-coded to the CloudSaw repo at
-- the binary level (see `github::CLOUDSAW_REPO`) — it isn't user-
-- configurable.
INSERT INTO settings (key, value, updated_at)
VALUES (
    'github_findings_repo',
    '',
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
);
