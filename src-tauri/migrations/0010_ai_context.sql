-- Migration 0010 — AI Suggestion Layer (Contract 13).
--
-- The AI suggestion layer is fully OPT-IN: nothing here grants
-- CloudSaw the ability to call any provider. The user must connect
-- their own provider API key (stored ONLY in the OS keychain at
-- `cloudsaw.llm_api_key`) AND explicitly confirm the request-preview
-- modal before any network call occurs.
--
-- This migration adds the structured business-context fields the
-- request builder reads from. They are NON-secret configuration:
--
--   * `ai_provider`         — `anthropic` | `openai` | empty (= disabled)
--   * `ai_context_industry` — short free-form tag (e.g. "fintech",
--                             "healthcare", "logistics"). The UI flags
--                             non-empty values with a "this will be sent
--                             to your AI provider" warning.
--   * `ai_context_environment_type` — `production` | `dev_test` | `mixed`
--   * `ai_context_compliance` — comma-separated framework tags
--                               (e.g. "PCI,SOC2,HIPAA").
--   * `ai_context_risk_tolerance`  — `low` | `medium` | `high`
--   * `ai_context_team_size`       — `solo` | `small` | `medium` | `large`
--
-- The defaults below seed a "disabled" state: no provider chosen, no
-- context fields set. Settings → AI is the only edit path.
--
-- NO request/response content is mirrored into SQLite — the event log
-- records that a request occurred (Contract 11), never its content
-- (CLAUDE.md §4.4 + Contract 13 §Constraints).

INSERT INTO settings (key, value, updated_at) VALUES
    ('ai_provider',                  '', strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    ('ai_context_industry',          '', strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    ('ai_context_environment_type',  '', strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    ('ai_context_compliance',        '', strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    ('ai_context_risk_tolerance',    '', strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    ('ai_context_team_size',         '', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
