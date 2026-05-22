// Maps a thrown IPC error onto a localized, user-safe string.
//
// The backend returns `{ code, message }`. We translate well-known codes
// through the locale dictionary so the UI shows consistent copy and never
// leaks raw Rust messages. Unknown codes fall back to the generic error key.
//
// Critically, the *same* localized string is used for "password rejected" and
// for any other unlock failure mode so the UI cannot accidentally distinguish
// a near-miss password from a totally wrong one (Contract 02 constraint).

import { useCallback } from "react";

import type { IpcError } from "@/lib/ipc";
import { useT } from "./useT";

function isIpcError(x: unknown): x is IpcError {
  return (
    typeof x === "object" &&
    x !== null &&
    "code" in x &&
    typeof (x as { code: unknown }).code === "string"
  );
}

const KNOWN_CODES: Record<string, string> = {
  invalid_input: "applock.error.invalid_input",
  password_rejected: "applock.error.unlock_failed",
  hash_error: "applock.error.generic",
  locked: "applock.error.generic",
  not_configured: "applock.error.not_configured",
  already_configured: "applock.error.already_configured",
  biometric_error: "applock.error.unlock_failed",
  biometric_unavailable: "applock.error.biometric_unavailable",
  identity_verification_error: "applock.error.identity_failed",
  identity_verification_unavailable: "applock.error.identity_unavailable",
  db_error: "applock.error.generic",
  io_error: "applock.error.generic",
  migration_error: "applock.error.generic",
  config_error: "applock.error.generic",
  path_error: "applock.error.generic",
  internal_error: "applock.error.generic",
  // AWS auth (Contract 03). Mapped to the dedicated `aws.error.*` keys so
  // the messaging is distinct from the app-lock surface.
  aws_config_unreadable: "aws.error.config_unreadable",
  profile_not_found: "aws.error.profile_not_found",
  aws_timeout: "aws.error.timeout",
  aws_connectivity: "aws.error.connectivity",
  aws_sso_expired: "aws.error.sso_expired",
  aws_permission_denied: "aws.error.permission_denied",
  // Multi-account (Contract 04). Distinct keys so the Accounts page can
  // localize each failure mode precisely.
  account_not_found: "accounts.error.not_found",
  duplicate_aws_account_id: "accounts.error.duplicate_aws_account_id",
  duplicate_label: "accounts.error.duplicate_label",
  aws_account_id_mismatch: "accounts.error.aws_account_id_mismatch",
  // Terraform scanner-role provisioner (Contract 05). Each variant maps to a
  // distinct localized message so the UI can guide the user to the right
  // remediation (reinstall vs. fix permissions vs. re-plan).
  terraform_not_bundled: "terraform.error.not_bundled",
  terraform_integrity_failed: "terraform.error.integrity_failed",
  terraform_init_failed: "terraform.error.init_failed",
  terraform_plan_failed: "terraform.error.plan_failed",
  terraform_apply_failed: "terraform.error.apply_failed",
  terraform_plan_token_invalid: "terraform.error.plan_token_invalid",
  terraform_plan_token_expired: "terraform.error.plan_token_expired",
  terraform_identity_unresolvable: "terraform.error.identity_unresolvable",
  terraform_trust_verification_failed: "terraform.error.trust_verification_failed",
  // Scanner orchestrator (Contract 06). Each scanner_* code maps to a
  // dedicated `scanner.failure.*` key so the UI can guide remediation
  // (reinstall vs. re-provision vs. re-run).
  scanner_not_bundled: "scanner.failure.scanner_not_bundled",
  scanner_integrity_failed: "scanner.failure.scanner_integrity_failed",
  scanner_role_not_provisioned: "scanner.failure.scanner_role_not_provisioned",
  scan_already_running: "scanner.failure.scan_already_running",
  scan_not_found: "scanner.failure.scan_not_found",
  scanner_assume_role_failed: "scanner.failure.scanner_assume_role_failed",
  scanner_spawn_failed: "scanner.failure.scanner_spawn_failed",
  scanner_process_lost: "scanner.failure.scanner_process_lost",
  scanner_process_failed: "scanner.failure.scanner_process_failed",
  scanner_output_missing: "scanner.failure.scanner_output_missing",
  // Event log, retention, hard delete & panic (Contract 11).
  confirmation_rejected: "eventlog.error.confirmation_rejected",
  schedule_not_found: "eventlog.error.generic",
};

export function useIpcError() {
  const t = useT();
  return useCallback(
    (err: unknown): string => {
      if (!isIpcError(err)) return t("common.error_generic");
      if (err.code === "rate_limited") {
        // Pull the wait seconds out of the backend message ("rate limited:
        // retry in 5s"). If we can't parse it, fall back to a generic.
        const match = err.message.match(/(\d+)\s*s/);
        const secs = match ? match[1] : "";
        return t("applock.error.rate_limited").replace("{seconds}", secs);
      }
      const key = KNOWN_CODES[err.code];
      return key ? t(key) : t("common.error_generic");
    },
    [t],
  );
}
