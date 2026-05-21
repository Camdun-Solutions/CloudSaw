// Single typed IPC client. Every component talks to the Rust backend
// through this module — no direct `invoke()` calls live in components, hooks,
// or routes (enforced by CONTRIBUTING.md and CI lint).
//
// Each method here corresponds 1:1 to a `#[tauri::command]` declared in
// src-tauri/src/ipc/mod.rs. Inputs and outputs are plain serializable shapes.

import { invoke } from "@tauri-apps/api/core";

/** Stable error shape returned by every backend command that can fail. */
export type IpcError = {
  code: string;
  message: string;
};

/** Re-lock cadence. The shape matches the Rust `LockPeriod` enum exactly so
 * `serde` round-trips it without an adapter. */
export type LockPeriod =
  | { kind: "immediate" }
  | { kind: "after"; seconds: number }
  | { kind: "never" };

export type BiometricAvailability = "Available" | "Unconfigured" | "Unavailable";

export type LockSettings = {
  lock_period: LockPeriod;
  biometric_enabled: boolean;
};

export type LockState = {
  first_run: boolean;
  locked: boolean;
  settings: LockSettings;
  biometric_availability: BiometricAvailability;
  recovery_available: boolean;
};

// --- AWS auth (Contract 03) ----------------------------------------------

/** Whether a profile is a vanilla AWS CLI profile or one backed by IAM
 * Identity Center (SSO). Drives a UI badge only — auth resolution uses
 * the SDK provider chain either way. */
export type ProfileSource = "cli" | "sso";

export type ProfileInfo = {
  name: string;
  source: ProfileSource;
};

/** Result of `sts:GetCallerIdentity`. Account/user IDs and ARN are
 * returned in full so the UI can confirm exactly which identity was
 * resolved — backend logs and error surfaces redact these values. */
export type CallerIdentity = {
  account_id: string;
  user_id: string;
  arn: string;
};

export type TestFailureReason =
  | "profile_not_configured"
  | "sso_expired"
  | "permission_denied"
  | "connectivity"
  | "timeout"
  | "other";

/** Discriminated union returned by `auth_test_profile`. Switch on `status`. */
export type ProfileTestResult =
  | { status: "success"; identity: CallerIdentity }
  | { status: "failure"; reason: TestFailureReason; api: string | null };

// --- Multi-account (Contract 04) -----------------------------------------

export type Environment = "dev" | "staging" | "prod" | "other";
export type ScanOutcome = "success" | "failure" | "partial_success" | "unknown";

/** One row of the local `accounts` table. The `aws_account_id` is the
 * verified 12-digit AWS account ID and serves as the partitioning key for
 * every account-scoped table added by later contracts. */
export type Account = {
  aws_account_id: string;
  label: string;
  profile_name: string;
  environment: Environment;
  role_provisioned: boolean;
  role_provisioned_at: string | null;
  last_scan_at: string | null;
  last_scan_status: ScanOutcome | null;
  created_at: string;
  updated_at: string;
};

export type AddAccountInput = {
  label: string;
  profile_name: string;
  environment: Environment;
};

export type UpdateAccountInput = {
  aws_account_id: string;
  label: string;
  profile_name: string;
  environment: Environment;
};

/** Data-impact preview returned from `accounts_remove`. `was_active` tells
 * the UI whether to prompt for a new active selection. */
export type RemovalImpact = {
  scans: number;
  findings: number;
  tf_work: number;
  was_active: boolean;
};

export type AccountsDisplaySettings = {
  reveal_full_ids: boolean;
};

/** Mask a 12-digit AWS account ID to the last 4 digits. Mirrors the Rust
 * `accounts::mask_for_logs` helper so the UI default and the log format
 * stay aligned. */
export function maskAccountId(id: string): string {
  if (id.length < 4) return "****";
  return `****${id.slice(-4)}`;
}

export const ipc = {
  /** CalVer build string, e.g. "2026.5.0". */
  appVersion(): Promise<string> {
    return invoke<string>("app_version");
  },

  // --- App lock ----------------------------------------------------------

  applockGetState(): Promise<LockState> {
    return invoke<LockState>("applock_get_state");
  },

  applockSetMasterPassword(password: string): Promise<void> {
    return invoke<void>("applock_set_master_password", { password });
  },

  applockUnlock(password: string): Promise<void> {
    return invoke<void>("applock_unlock", { password });
  },

  applockUnlockWithBiometric(reason: string): Promise<void> {
    return invoke<void>("applock_unlock_with_biometric", { reason });
  },

  applockLock(): Promise<void> {
    return invoke<void>("applock_lock");
  },

  applockChangePassword(oldPassword: string, newPassword: string): Promise<void> {
    return invoke<void>("applock_change_password", {
      oldPassword,
      newPassword,
    });
  },

  applockRecoveryUnlock(newPassword: string, reason: string): Promise<void> {
    return invoke<void>("applock_recovery_unlock", { newPassword, reason });
  },

  applockGetSettings(): Promise<LockSettings> {
    return invoke<LockSettings>("applock_get_settings");
  },

  applockSetSettings(settings: LockSettings): Promise<void> {
    return invoke<void>("applock_set_settings", { settings });
  },

  applockVerifyPassword(password: string): Promise<boolean> {
    return invoke<boolean>("applock_verify_password", { password });
  },

  // --- AWS auth --------------------------------------------------------

  authListProfiles(): Promise<ProfileInfo[]> {
    return invoke<ProfileInfo[]>("auth_list_profiles");
  },

  authGetCallerIdentity(profile: string): Promise<CallerIdentity> {
    return invoke<CallerIdentity>("auth_get_caller_identity", { profile });
  },

  authTestProfile(profile: string): Promise<ProfileTestResult> {
    return invoke<ProfileTestResult>("auth_test_profile", { profile });
  },

  // --- Multi-account ----------------------------------------------------

  accountsList(): Promise<Account[]> {
    return invoke<Account[]>("accounts_list");
  },

  accountsGet(awsAccountId: string): Promise<Account> {
    return invoke<Account>("accounts_get", { awsAccountId });
  },

  accountsAdd(input: AddAccountInput): Promise<Account> {
    return invoke<Account>("accounts_add", { input });
  },

  accountsUpdate(input: UpdateAccountInput): Promise<Account> {
    return invoke<Account>("accounts_update", { input });
  },

  accountsRemove(awsAccountId: string): Promise<RemovalImpact> {
    return invoke<RemovalImpact>("accounts_remove", { awsAccountId });
  },

  accountsGetActive(): Promise<string | null> {
    return invoke<string | null>("accounts_get_active");
  },

  accountsSetActive(awsAccountId: string | null): Promise<void> {
    return invoke<void>("accounts_set_active", { awsAccountId });
  },

  accountsGetDisplaySettings(): Promise<AccountsDisplaySettings> {
    return invoke<AccountsDisplaySettings>("accounts_get_display_settings");
  },

  accountsSetDisplaySettings(
    settings: AccountsDisplaySettings,
  ): Promise<void> {
    return invoke<void>("accounts_set_display_settings", { settings });
  },
};

export type Ipc = typeof ipc;
