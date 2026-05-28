import { useCallback, useEffect, useState } from "react";

import {
  ErrorBoundary,
  ErrorReportDialog,
  TopNav,
  UpdateBanner,
  VersionFooter,
  type TopNavRoute,
} from "@/components";
import { ScanModalProvider, SCAN_FINISHED_EVENT } from "@/contexts/ScanModalContext";
import { notifyScanComplete } from "@/lib/scanNotifications";
import { useAppearance } from "@/hooks/useAppearance";
import { useT } from "@/hooks/useT";
// PR #46: Accounts is no longer a top-level route — it's an
// embedded section inside Settings. App.tsx doesn't render it
// directly anymore; Settings imports it.
import ActivityLog from "@/routes/ActivityLog";
import CustomReport from "@/routes/CustomReport";
import Dashboard from "@/routes/Dashboard";
import Findings from "@/routes/Findings";
import Home from "@/routes/Home";
import Onboarding from "@/routes/Onboarding";
import Profiles from "@/routes/Profiles";
import ScheduledScans from "@/routes/ScheduledScans";
import Settings from "@/routes/Settings";
import UnlockScreen from "@/routes/UnlockScreen";
import { ipc, type OnboardingState } from "@/lib/ipc";
import { useLock } from "@/stores/lock";

type Route =
  // "accounts" was a top-level route until PR #46; it's now an
  // embedded section inside `Settings`. The Route union no longer
  // includes it — every former caller (Dashboard, Home, etc.)
  // routes to "settings" instead.
  | "home"
  | "profiles"
  | "settings"
  | "schedules"
  | "activitylog"
  | "custom_report"
  | "dashboard"
  // "findings" deep-links into the Dashboard component with
  // `initialTab="findings"`. PR #48 (Findings overhaul) will promote
  // this to its own page and remove the Dashboard sub-tab.
  | "findings";

/** Map the parent `Route` union onto the subset the persistent TopNav
 *  knows about. Returns `null` while on a "deeper" route (Profiles,
 *  etc.) so no menu button is shown active — the user is somewhere
 *  intermediate. */
function topNavActive(route: Route): TopNavRoute | null {
  switch (route) {
    case "home":
    case "dashboard":
      return "dashboard";
    case "findings":
      return "findings";
    case "settings":
    case "schedules":
    case "activitylog":
    case "custom_report":
      return "settings";
    case "profiles":
      return null;
  }
}

export default function App() {
  const t = useT();
  // PR #57: keep `<html class="dark">` in sync with the user's
  // Settings → Appearance choice + the OS prefers-color-scheme media
  // query when in "system" mode. The hook itself does not return any
  // render-affecting state at the root; Settings reads it via its own
  // `useAppearance()` invocation to drive the radio control.
  useAppearance();
  const { status, state, error, refresh } = useLock();
  const [route, setRoute] = useState<Route>("home");
  // Manual-open path for the error dialog. Wired into the lock-load
  // error fallback below and into the ErrorBoundary, so any failure
  // path can reach the bug-report flow.
  const [errorDialogOpen, setErrorDialogOpen] = useState(false);
  const [errorDialogNotes, setErrorDialogNotes] = useState<string | undefined>(
    undefined,
  );

  // Onboarding state — Contract 14. The wizard is the only entry point
  // until `completed = true`. We hydrate once at mount and refresh
  // after the wizard finishes so the App re-renders into the main app.
  const [onboarding, setOnboarding] = useState<OnboardingState | null>(null);
  const [onboardingLoading, setOnboardingLoading] = useState(true);

  const refreshOnboarding = useCallback(async () => {
    try {
      setOnboarding(await ipc.onboardingGetState());
    } catch {
      // Fail-open: a stalled IPC must not strand the user behind the
      // wizard. The error path below renders the bug-report affordance.
      setOnboarding(null);
    } finally {
      setOnboardingLoading(false);
    }
  }, []);

  useEffect(() => {
    void refreshOnboarding();
  }, [refreshOnboarding]);

  // PR #54 — desktop notification on scan completion. Listens for
  // the global SCAN_FINISHED_EVENT (fired by ScanModalProvider on
  // scan-modal completions, plus Accounts.tsx for legacy callsites
  // — see PR #54 for the dispatch wiring). The helper itself gates
  // on the user's Settings → Notifications opt-in toggle, so this
  // listener is harmless when the user hasn't enabled.
  useEffect(() => {
    const handler = () => {
      void notifyScanComplete({
        title: t("notifications.scan_complete.title"),
        body: t("notifications.scan_complete.body"),
      });
    };
    document.addEventListener(SCAN_FINISHED_EVENT, handler);
    return () => document.removeEventListener(SCAN_FINISHED_EVENT, handler);
  }, [t]);

  // PR #64 — DEV-ONLY: expose `seedDemoFindings` on the
  // `__cloudsaw_dev` console namespace so a developer running
  // `npm run tauri dev` against a fresh data root can populate the
  // Findings UI without a real AWS scan / a vendored ScoutSuite
  // binary. The Rust handler gates its body on `cfg(debug_assertions)`
  // and the entire useEffect is dead-code-eliminated from release
  // bundles by Vite via the `import.meta.env.DEV` check. Merges
  // into any existing `__cloudsaw_dev` (e.g. the locale hook's
  // entries) so neither overwrites the other.
  //
  // Usage in the browser DevTools console:
  //   await __cloudsaw_dev.seedDemoFindings()            // active account
  //   await __cloudsaw_dev.seedDemoFindings("123456789012")
  useEffect(() => {
    if (!import.meta.env.DEV) return;
    const w = window as unknown as { __cloudsaw_dev?: Record<string, unknown> };
    w.__cloudsaw_dev = {
      ...w.__cloudsaw_dev,
      seedDemoFindings: async (awsAccountId?: string) => {
        let target = awsAccountId;
        if (!target) {
          const active = await ipc.accountsGetActive();
          if (!active) {
            throw new Error(
              "No active account — pass an awsAccountId or set one via Settings → Accounts",
            );
          }
          target = active;
        }
        return ipc.devSeedDemoFindings(target);
      },
    };
  }, []);

  function openReport(notes?: string) {
    setErrorDialogNotes(notes);
    setErrorDialogOpen(true);
  }

  if (status === "loading" || onboardingLoading) {
    return (
      <main className="min-h-full bg-saw-grey-50 dark:bg-saw-black flex items-center justify-center">
        <p className="text-body text-saw-grey-600 dark:text-saw-grey-400">{t("common.loading")}</p>
      </main>
    );
  }

  if (status === "error" || !state) {
    return (
      <>
        <main className="min-h-full bg-saw-grey-50 dark:bg-saw-black flex items-center justify-center px-6 py-12">
          <div className="max-w-md text-center">
            <p
              role="alert"
              className="rounded-card bg-saw-white dark:bg-saw-grey-dark border border-saw-grey-200 dark:border-saw-grey-700 px-4 py-3 text-body text-saw-red"
            >
              {error ?? t("common.error_generic")}
            </p>
            <div className="mt-4 flex items-center justify-center gap-4">
              <button
                type="button"
                onClick={() => void refresh()}
                className="text-small text-saw-grey-700 dark:text-saw-grey-300 underline underline-offset-2"
              >
                {t("common.confirm")}
              </button>
              <button
                type="button"
                onClick={() => openReport(error ?? undefined)}
                className="text-small text-saw-grey-700 dark:text-saw-grey-300 underline underline-offset-2"
                data-testid="lock-error-report"
              >
                {t("errordialog.file_bug")}
              </button>
            </div>
          </div>
        </main>
        <ErrorReportDialog
          open={errorDialogOpen}
          initialNotes={errorDialogNotes}
          onClose={() => setErrorDialogOpen(false)}
          onConfigureToken={() => {
            setErrorDialogOpen(false);
            // Can't navigate into Settings from the locked-error fallback
            // because the app isn't loaded — the user retries first.
          }}
        />
      </>
    );
  }

  // The onboarding wizard subsumes the original first-run gate from
  // Contract 02 — its password step is the same flow FirstRunSetup
  // used to drive. With the wizard in place there is no longer a
  // standalone first-run screen.
  //
  // Lock screen only when the password IS set AND the session is
  // locked. With no password set, the wizard handles step 2.
  if (state.locked) return <UnlockScreen />;

  if (!onboarding?.completed) {
    return (
      // PR #52: ScanModalProvider also wraps onboarding so the
      // FirstScanStep's `useScanModal()` hook + the global
      // SCAN_FINISHED_EVENT listener work inside the wizard.
      // Two separate provider instances (one here, one in the
      // post-onboarding branch below) is fine — they're never
      // active simultaneously.
      <ScanModalProvider>
        <Onboarding
          onCompleted={(landingRoute) => {
            // PR #52: if the wizard requested a specific landing
            // page (e.g. FirstScanStep → "findings" after the
            // bootstrap scan completes), flip the route BEFORE
            // re-hydrating onboarding state so the user lands
            // there immediately rather than briefly seeing the
            // default Dashboard.
            if (landingRoute) setRoute(landingRoute);
            void refreshOnboarding();
          }}
        />
      </ScanModalProvider>
    );
  }

  return (
    <>
      {/* Auto-updater banner (Contract 16C). Renders only when a
        verified update is available. The component is dynamic-import
        gated so the browser preview (no Tauri runtime) doesn't crash
        on the plugin import. */}
      <UpdateBanner />
      <ErrorBoundary
        fallback={({ errorMessage, clear }) => (
        <>
          <main className="min-h-full bg-saw-grey-50 dark:bg-saw-black flex items-center justify-center px-6 py-12">
            <div className="max-w-md text-center">
              <p
                role="alert"
                className="rounded-card bg-saw-white dark:bg-saw-grey-dark border border-saw-grey-200 dark:border-saw-grey-700 px-4 py-3 text-body text-saw-red"
                data-testid="render-error-message"
              >
                {errorMessage}
              </p>
              <div className="mt-4 flex items-center justify-center gap-4">
                <button
                  type="button"
                  onClick={clear}
                  className="text-small text-saw-grey-700 dark:text-saw-grey-300 underline underline-offset-2"
                >
                  {t("errordialog.dismiss")}
                </button>
                <button
                  type="button"
                  onClick={() => openReport(errorMessage)}
                  className="text-small text-saw-grey-700 dark:text-saw-grey-300 underline underline-offset-2"
                  data-testid="render-error-report"
                >
                  {t("errordialog.file_bug")}
                </button>
              </div>
            </div>
          </main>
          <ErrorReportDialog
            open={errorDialogOpen}
            initialNotes={errorDialogNotes}
            onClose={() => setErrorDialogOpen(false)}
            onConfigureToken={() => {
              setErrorDialogOpen(false);
              clear();
              setRoute("settings");
            }}
          />
        </>
      )}
    >
      <ScanModalProvider>
        {/* Persistent top-right menu — Dashboard / Findings /
            Settings. Fixed-positioned so it overlays whatever route
            renders below. PR #41 introduces it; PR #42 will add the
            lock icon to the right side, PR #44 will remove the
            duplicate per-route nav buttons in Home/Dashboard/etc.
            once this nav is verified. */}
        <TopNav
          active={topNavActive(route)}
          onNavigate={(target) => {
            // PR #50: "Dashboard" in the TopNav lands on the new
            // Welcome content rendered by Home.tsx. The "dashboard"
            // Route value still exists in the union for backward
            // compat with the legacy Dashboard.tsx (scans / drift /
            // trends tabs) that's still reachable via the
            // "findings" route's initialTab seam — but no UI
            // surface routes there directly anymore.
            setRoute(target === "dashboard" ? "home" : target);
          }}
          onLock={() => {
            // The lock state listener in the useLock store flips
            // status to "locked" on the next IPC tick; App.tsx
            // re-renders into <UnlockScreen /> via the `state.locked`
            // gate above. Errors here would mean the lock IPC
            // rejected (rare — usually only if SQLite is hosed), in
            // which case the ErrorBoundary picks up the throw.
            void ipc.applockLock();
          }}
        />
        {/* Global version footer (PR #43). Fixed bottom-left so the
            version is always visible regardless of route. Hidden
            automatically while locked / during onboarding because
            this render branch sits below those gates. */}
        <VersionFooter />
        <AppShell
          route={route}
          setRoute={setRoute}
          onOpenReport={openReport}
          onRerunOnboarding={async (startAt) => {
            try {
              await ipc.onboardingResetForRerun(startAt);
              await refreshOnboarding();
            } catch {
              // Surface via the bug-report path so the user sees an
              // actionable affordance rather than a silent failure.
              openReport("Failed to re-enter the onboarding wizard.");
            }
          }}
        />
        <ErrorReportDialog
          open={errorDialogOpen}
          initialNotes={errorDialogNotes}
          onClose={() => setErrorDialogOpen(false)}
          onConfigureToken={() => {
            setErrorDialogOpen(false);
            setRoute("settings");
          }}
        />
      </ScanModalProvider>
      </ErrorBoundary>
    </>
  );
}

function AppShell({
  route,
  setRoute,
  onOpenReport,
  onRerunOnboarding,
}: {
  route: Route;
  setRoute: (r: Route) => void;
  onOpenReport: (notes?: string) => void;
  onRerunOnboarding: (startAt: "aws_account" | "language") => void;
}) {
  if (route === "settings") {
    return (
      <Settings
        onClose={() => setRoute("home")}
        onOpenSchedules={() => setRoute("schedules")}
        onOpenActivityLog={() => setRoute("activitylog")}
        onOpenCustomReport={() => setRoute("custom_report")}
        onOpenProfiles={() => setRoute("profiles")}
        onRerunOnboarding={onRerunOnboarding}
      />
    );
  }
  if (route === "schedules") {
    return <ScheduledScans onBack={() => setRoute("settings")} />;
  }
  if (route === "activitylog") {
    return <ActivityLog onBack={() => setRoute("settings")} />;
  }
  if (route === "custom_report") {
    return <CustomReport onBack={() => setRoute("settings")} />;
  }
  if (route === "profiles") {
    // PR #46: Accounts is now an embedded section inside Settings.
    // Profiles still has its own page; the Back button returns to
    // Settings (which is where the user opened it from via the
    // embedded Accounts panel's "Open profiles" button).
    return <Profiles onClose={() => setRoute("settings")} />;
  }
  if (route === "dashboard") {
    return (
      <Dashboard
        onClose={() => setRoute("home")}
        onOpenAccounts={() => setRoute("settings")}
        onOpenReport={onOpenReport}
      />
    );
  }
  if (route === "findings") {
    // PR #51: Findings promoted to a first-class top-level page.
    // The legacy Dashboard.tsx still has a findings tab and is
    // still reachable via the "dashboard" route, but the TopNav
    // Findings button now lands on the new page.
    return <Findings onBack={() => setRoute("home")} />;
  }
  return (
    <Home onOpenSettings={() => setRoute("settings")} />
  );
}
