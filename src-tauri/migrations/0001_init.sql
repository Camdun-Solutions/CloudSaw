-- Migration 0001 — initial schema.
--
-- Scope is intentionally minimal in Contract 01: this migration only proves
-- the runner end-to-end. Real domain tables (findings, scans, events) land in
-- their owning contracts (07, 09, 11) so each contract reviews its own schema.

CREATE TABLE settings (
    key         TEXT PRIMARY KEY,
    value       TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

INSERT INTO settings (key, value, updated_at)
VALUES ('schema_initialized', '1', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
