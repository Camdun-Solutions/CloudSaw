-- Migration 0014 — finding_resources gains identity columns.
--
-- The original schema (0006) only stored each resource as a dotted
-- ScoutSuite path: `iam.users.AIDA5QH5NBNECJ6OXSWUT.mfa_enabled`. That
-- path uses the immutable AWS principal id, not the human-readable
-- name, so the UI surfaced an unreadable token instead of the actual
-- user / role / policy / bucket the finding affects.
--
-- PR #82 enriches the parser to extract the deepest "resource entity"
-- ancestor of each ScoutSuite item path (the level that carries
-- `name` / `arn` / `id` scalars) and persist three identity columns
-- + a forward-compat `attributes_json` blob:
--
--   * `resource_name`     — human-readable identifier
--                           (`cloudsaw-service-role`, `prod-db-1`, etc.)
--   * `resource_arn`      — full AWS ARN when ScoutSuite captures one
--   * `resource_id_value` — the AWS id (AIDA/AROA/AKIA/ANPA/etc.) so
--                           the UI can carry it as a stable lookup
--                           even after a rename. Suffix is `_value`
--                           because plain `resource_id` collides with
--                           the table's existing INTEGER PRIMARY KEY.
--   * `attributes_json`   — JSON object holding every other scalar
--                           field captured at the resource entity
--                           level (CreateDate, AccessKeys count,
--                           tags, etc.). The frontend renders every
--                           non-null key by default; trim happens in
--                           the UI, not at the storage boundary.
--
-- All four columns are nullable so:
--   * Old rows persisted by pre-0014 builds keep working — they read
--     as nulls, the frontend falls back to the dotted path.
--   * Findings whose ScoutSuite item path doesn't land on an entity
--     with identifying fields (e.g. password_policy globals) also
--     read as nulls — same fallback.

ALTER TABLE finding_resources ADD COLUMN resource_name TEXT;
ALTER TABLE finding_resources ADD COLUMN resource_arn TEXT;
ALTER TABLE finding_resources ADD COLUMN resource_id_value TEXT;
ALTER TABLE finding_resources ADD COLUMN attributes_json TEXT;
