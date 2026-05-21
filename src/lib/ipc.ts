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
};

export type Ipc = typeof ipc;
