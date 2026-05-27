// Desktop notification helper for scan completion — PR #54.
//
// Wraps `@tauri-apps/plugin-notification` with a CloudSaw-specific
// gate: notifications only fire when the user has opted in via the
// Settings → Notifications toggle (default OFF — privacy-first).
//
// The toggle is persisted to localStorage (key:
// `cloudsaw.scanNotificationsEnabled`) — same pattern the
// UpdatesSection already uses for the "auto-check on launch" pref.
// Backend persistence would be cleaner but localStorage is enough
// for a user-facing UI pref that doesn't need cross-device sync.
//
// Permission flow:
//   - First `sendNotification` call on macOS triggers the system
//     permission prompt. If denied, the call silently no-ops; we
//     don't surface the rejection in-app because it's already
//     loud via the OS dialog.
//   - Windows + Linux: no permission prompt; just fires.
//
// The helper is dynamic-import gated so the browser preview (no
// Tauri runtime) doesn't crash on the plugin import — same
// pattern UpdateBanner uses.

const STORAGE_KEY = "cloudsaw.scanNotificationsEnabled";

/** Read the user's opt-in preference. Defaults to false — the user
 *  must explicitly enable in Settings before any OS notification
 *  fires. */
export function isScanNotificationsEnabled(): boolean {
  try {
    return localStorage.getItem(STORAGE_KEY) === "true";
  } catch {
    return false;
  }
}

/** Persist the opt-in preference. */
export function setScanNotificationsEnabled(enabled: boolean): void {
  try {
    localStorage.setItem(STORAGE_KEY, enabled ? "true" : "false");
  } catch {
    // Silent fail — localStorage write errors are exotic and there's
    // no useful UI surface to communicate them.
  }
}

/** Fire a desktop notification for a completed scan. No-op when:
 *   - the user hasn't opted in (default state)
 *   - the Tauri plugin import fails (browser preview / dev shell)
 *   - the OS permission check returns false
 *
 * Errors are caught + silently dropped — a misfired notification
 * shouldn't interrupt the scan-complete UI flow. */
export async function notifyScanComplete(opts: {
  title: string;
  body: string;
}): Promise<void> {
  if (!isScanNotificationsEnabled()) return;
  try {
    const plugin = await import("@tauri-apps/plugin-notification");
    let granted = await plugin.isPermissionGranted();
    if (!granted) {
      const perm = await plugin.requestPermission();
      granted = perm === "granted";
    }
    if (!granted) return;
    await plugin.sendNotification({
      title: opts.title,
      body: opts.body,
    });
  } catch {
    // Plugin not loadable (no Tauri runtime) or OS-level failure —
    // silent. The in-app dashboard refresh still happens via the
    // SCAN_FINISHED_EVENT listener; the desktop notification is
    // best-effort additional sugar.
  }
}
