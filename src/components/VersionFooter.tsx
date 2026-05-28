// Global fixed-position app-version footer.
//
// Renders in the bottom-left corner of every authenticated page
// (anything below the lock + onboarding gates in App.tsx). Replaces
// the old per-Home-header version badge — moved here so the version
// stays visible regardless of which route the user is on, freeing
// the page chrome to focus on per-route content.
//
// Style intentionally subdued (small, low-contrast grey) so it
// fades into the background until the user looks for it. Click
// targets are NOT here — this is informational only.

import { useEffect, useState } from "react";

import { useT } from "@/hooks/useT";
import { ipc } from "@/lib/ipc";

export default function VersionFooter() {
  const t = useT();
  const [version, setVersion] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    ipc
      .appVersion()
      .then((v) => {
        if (!cancelled) setVersion(v);
      })
      .catch(() => {
        // Silent: a version-fetch failure shouldn't surface to the
        // user via a UI string. The footer just stays empty until
        // the next IPC works. If `ipc.appVersion()` is structurally
        // broken (no Tauri runtime, etc.) we have bigger issues
        // that the ErrorBoundary picks up at a real surface.
        if (!cancelled) setVersion(null);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  if (!version) return null;

  return (
    <div
      // PR #68: shifted right from `left-2` to `left-10` so the
      // ReportBugFlag (also at `bottom-2 left-2`, 24px wide) sits
      // immediately to the left without overlap. Z-index stays lower
      // than the flag so the flag's focus ring overlaps cleanly.
      className="pointer-events-none fixed bottom-2 left-10 z-30 text-xs text-saw-grey-500 dark:text-saw-grey-400"
      data-testid="version-footer"
      aria-label={`${t("app.version_label")} ${version}`}
    >
      {t("app.version_label")} {version}
    </div>
  );
}
