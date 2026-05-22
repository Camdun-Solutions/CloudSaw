-- Migration 0012 — Report Exporter (Contract 15).
--
-- Three rows in the existing `settings` table; non-secret configuration
-- only.
--
--   * `report_auto_export_folder` — absolute path the user picked
--     for auto-export, or empty when the feature is disabled.
--   * `report_auto_export_enabled` — `1` or `0` (independent toggle so
--     the user can quickly disable without losing the folder).
--   * `report_mask_account_ids_default` — `1` (default) or `0`. The
--     export flow lets the user opt-in to full account IDs on a
--     per-export basis; this row is the default.
--
-- NO report file paths, NO scan IDs, NO account identifiers are mirrored
-- here. The reports module produces files in the chosen folder and the
-- event-log entry records that an export happened, not its content
-- (Contract 11 + 15 §Constraints).

INSERT INTO settings (key, value, updated_at) VALUES
    ('report_auto_export_folder',       '',  strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    ('report_auto_export_enabled',      '0', strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    ('report_mask_account_ids_default', '1', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
