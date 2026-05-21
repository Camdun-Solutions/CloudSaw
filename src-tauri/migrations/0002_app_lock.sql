-- Migration 0002 — application lock (Contract 02).
--
-- Stores the master-password state and lock-session settings.
--
-- Design notes (CLAUDE.md §4.3 + Contract 02 constraints):
--   * `password_hash` holds the Argon2id PHC string. An Argon2id hash is not a
--     secret value (it's irreversible by construction), so it is permitted in
--     SQLite. Plaintext or any reversible form of the password is never stored.
--   * No biometric secret material lives here. Biometric unlock relies on the
--     OS prompt's pass/fail result — we never store a token we'd have to
--     defend against extraction.
--   * `lock_period_seconds = NULL` encodes the "never re-lock" option; positive
--     integers encode timed re-lock; `0` encodes "lock immediately on close".
--   * Only one row ever exists in this table (id=1 invariant).

CREATE TABLE app_lock (
    id                    INTEGER PRIMARY KEY CHECK (id = 1),
    password_hash         TEXT,
    biometric_enabled     INTEGER NOT NULL DEFAULT 0,
    lock_period_seconds   INTEGER,
    last_unlocked_at      TEXT,
    updated_at            TEXT NOT NULL
);

-- Default row. `lock_period_seconds = 604800` is 7 days (Contract 02 default).
-- `password_hash` is NULL until the user completes first-run setup.
INSERT INTO app_lock (id, password_hash, biometric_enabled, lock_period_seconds, updated_at)
VALUES (1, NULL, 0, 604800, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
