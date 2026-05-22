-- Migration 0011 — Onboarding Wizard (Contract 14).
--
-- One singleton table. Stores ONLY:
--   * the step the user is currently on,
--   * per-step completion flags,
--   * the chosen UI language,
--   * a single `completed` flag flipped when the whole wizard ends.
--
-- This is Contract 14 §Constraints: "The wizard MUST store no credentials
-- and no account-identifying information beyond what the underlying
-- modules already persist; its own state is just step-completion flags
-- and chosen language."
--
-- Step IDs are 1..6 matching the contract's Expected Output:
--   1 = language
--   2 = master_password
--   3 = aws_account
--   4 = terraform
--   5 = business_context  (optional)
--   6 = first_scan
--
-- `current_step` is the step the user is currently on (or returning to
-- after a quit). `step_*_completed` flags are set the moment the user
-- finishes a step — they are how the wizard decides whether to resume.

CREATE TABLE onboarding_state (
    id                            INTEGER PRIMARY KEY CHECK (id = 1),
    completed                     INTEGER NOT NULL DEFAULT 0,
    current_step                  INTEGER NOT NULL DEFAULT 1,
    language                      TEXT NOT NULL DEFAULT 'en',
    step_language_completed       INTEGER NOT NULL DEFAULT 0,
    step_password_completed       INTEGER NOT NULL DEFAULT 0,
    step_account_completed        INTEGER NOT NULL DEFAULT 0,
    step_terraform_completed      INTEGER NOT NULL DEFAULT 0,
    step_context_completed        INTEGER NOT NULL DEFAULT 0,
    step_first_scan_completed     INTEGER NOT NULL DEFAULT 0,
    completed_at                  TEXT,
    updated_at                    TEXT NOT NULL
);

INSERT INTO onboarding_state (id, updated_at)
VALUES (1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
