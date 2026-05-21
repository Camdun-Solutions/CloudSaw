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

export const ipc = {
  /** CalVer build string, e.g. "2026.5.0". */
  appVersion(): Promise<string> {
    return invoke<string>("app_version");
  },
};

export type Ipc = typeof ipc;
