// Update-notification preferences.
//
// The "auto-check on launch" toggle lives in localStorage rather than
// SQLite because (a) it's not security-sensitive — the worst a tampered
// value can do is suppress an opt-in banner — and (b) re-running the
// onboarding migration just to add a single boolean would force every
// install to take a SQLite migration on next launch.
//
// Default: enabled. Users who silence updates do so deliberately, and
// the manual "Check for updates" button in Settings → Updates remains
// available regardless of this preference.

const AUTO_CHECK_KEY = "cloudsaw.update.auto_check";

export function getAutoCheckEnabled(): boolean {
  if (typeof window === "undefined") return true;
  try {
    const raw = window.localStorage.getItem(AUTO_CHECK_KEY);
    // Treat any unset / malformed value as "enabled" so a fresh install
    // sees the banner on first launch.
    if (raw === null) return true;
    return raw !== "false";
  } catch {
    return true;
  }
}

export function setAutoCheckEnabled(next: boolean): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(AUTO_CHECK_KEY, next ? "true" : "false");
    // Notify same-tab listeners — the storage event only fires on OTHER
    // tabs/windows. We dispatch a synthetic event so the UpdateBanner
    // (which mounted before the user opened Settings) can react.
    window.dispatchEvent(new CustomEvent("cloudsaw:update-prefs-changed"));
  } catch {
    // Storage may be unavailable (private mode, quota exceeded). Falling
    // through means the next launch picks up the default — acceptable
    // for a UI preference.
  }
}

/** GitHub release page for a given version tag. Used by the Settings
 *  page's "Release notes" link so users can review what changed before
 *  installing. Stable URL even if `latest.json#notes` is empty. */
export function releaseNotesUrl(version: string): string {
  return `https://github.com/Camdun-Solutions/CloudSaw/releases/tag/${encodeURIComponent(version)}`;
}
