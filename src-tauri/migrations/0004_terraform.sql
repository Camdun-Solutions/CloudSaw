-- Migration 0004 — Terraform scanner-role provisioning (Contract 05).
--
-- Adds columns the accounts table needs to track scanner-role state without
-- inventing a new table. Everything here is configuration (no credentials,
-- no STS tokens, CLAUDE.md §4.3) — `external_id` is a CloudSaw-generated
-- random string used as a confused-deputy guard on the role's trust policy,
-- not a credential.
--
-- `external_id` is stored alongside the account because future contracts
-- (06: scanner orchestrator) need it when assuming the role. It's specific
-- to the role-trust relationship, not a secret in the credential sense.
--
-- `policy_variant` is one of {"security_audit","read_only_access"} and
-- records which AWS-managed policy was attached. Default at insert time is
-- NULL (no provisioning attempted yet); plan/apply writes it.
--
-- `last_provisioning_error` captures the most recent provisioning failure
-- as a stable error tag (e.g. "permission_denied", "binary_integrity_failed",
-- "apply_failed") for UI surfacing. Never carries raw Terraform stderr.

ALTER TABLE accounts ADD COLUMN external_id TEXT;
ALTER TABLE accounts ADD COLUMN policy_variant TEXT;
ALTER TABLE accounts ADD COLUMN last_provisioning_error TEXT;
ALTER TABLE accounts ADD COLUMN scanner_role_arn TEXT;
