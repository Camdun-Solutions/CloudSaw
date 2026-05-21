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
