-- Migration 0013 — Multi-provider AI (PR #74).
--
-- Replaces the single-AI-provider model from migration 0010 with a
-- table that holds one row per connected provider. Each row carries a
-- random `provider_id`, a user-chosen `nickname`, the `provider_type`
-- (anthropic | openai), and the last four characters of the key for
-- UI display ("****ABCD"). THE REAL KEY NEVER ENTERS THIS TABLE —
-- per Contract 13 §Constraints, the key lives only in the OS keychain,
-- under `cloudsaw.llm_api_key` with `account = provider_id`.
--
-- One provider can be the "active" one at any moment, recorded in the
-- `ai_active_provider_id` settings row. Clearing that value (empty
-- string) returns the layer to the dormant state.
--
-- Migration shape:
--   * The legacy `ai_provider` setting (anthropic | openai | '') is
--     translated into a provider row with `provider_id = provider_type`
--     so the existing keychain slot (account = 'anthropic' | 'openai')
--     keeps working — no key transfer needed. The nickname defaults to
--     the human form of the provider name; users can rename in
--     Settings.
--   * `ai_active_provider_id` is initialized to the legacy provider's
--     id so the user's active provider survives the upgrade.
--   * The legacy `ai_provider` setting is kept (not dropped) so a
--     downgrade-to-2026.5.31 reads it correctly. New writes go through
--     the providers table only.

CREATE TABLE IF NOT EXISTS ai_providers (
    provider_id    TEXT PRIMARY KEY,
    provider_type  TEXT NOT NULL,
    nickname       TEXT NOT NULL,
    key_last4      TEXT NOT NULL DEFAULT '',
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_ai_providers_provider_type
    ON ai_providers (provider_type);

INSERT INTO settings (key, value, updated_at) VALUES
    ('ai_active_provider_id', '', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));

-- Backfill the legacy single provider, if any.
INSERT INTO ai_providers (provider_id, provider_type, nickname, key_last4, created_at, updated_at)
SELECT
    s.value,                                             -- provider_id == legacy account name
    s.value,                                             -- provider_type
    CASE s.value
        WHEN 'anthropic' THEN 'Anthropic'
        WHEN 'openai'    THEN 'OpenAI'
        ELSE s.value
    END,
    '',                                                  -- key_last4 unknown at migration time
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
FROM settings s
WHERE s.key = 'ai_provider'
  AND s.value IN ('anthropic', 'openai')
  AND NOT EXISTS (SELECT 1 FROM ai_providers p WHERE p.provider_id = s.value);

UPDATE settings
SET value = (SELECT value FROM settings WHERE key = 'ai_provider'),
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE key = 'ai_active_provider_id'
  AND (SELECT value FROM settings WHERE key = 'ai_provider') IN ('anthropic', 'openai');
