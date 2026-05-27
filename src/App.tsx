import { useCallback, useEffect, useState } from "react";

import {
  ErrorBoundary,
  ErrorReportDialog,
  TopNav,
  UpdateBanner,
  VersionFooter,
  type TopNavRoute,
} from "@/components";
import { ScanModalProvider } from "@/contexts/ScanModalContext";
import { useT } from "@/hooks/useT";
import Accounts from "@/routes/Accounts";
import ActivityLog from "@/routes/ActivityLog";
import CustomReport from "@/routes/CustomReport";
import Dashboard from "@/routes/Dashboard";
import Home from "@/routes/Home";
import Onboarding from "@/routes/Onboarding";
import Profiles from "@/routes/Profiles";
import ScheduledScans from "@/routes/ScheduledScans";
import Settings from "@/routes/Settings";
import UnlockScreen from "@/routes/UnlockScreen";
import { ipc, type OnboardingState } from "@/lib/ipc";
import { useLock } from "@/stores/lock";

type Route =
  | "home"
  | "accounts"
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
 *  knows about. Returns `null` while on a "deeper" route (Accounts,
 *  Profiles, Schedules, etc.) so no menu button is shown active —
 *  the user is somewhere intermediate. */
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
    case "accounts":
    case "profiles":
      return null;
  }
}

export default function App() {
  const t = useT();
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

  function openReport(notes?: string) {
    setErrorDialogNotes(notes);
    setErrorDialogOpen(true);
  }

  if (status === "loading" || onboardingLoading) {
    return (
      <main className="min-h-full bg-saw-grey-50 flex items-center justify-center">
        <p className="text-body text-saw-grey-600">{t("common.loading")}</p>
      </main>
    );
  }

  if (status === "error" || !state) {
    return (
      <>
        <main className="min-h-full bg-saw-grey-50 flex items-center justify-center px-6 py-12">
          <div className="max-w-md text-center">
            <p
              role="alert"
              className="rounded-card bg-saw-white border border-saw-grey-200 px-4 py-3 text-body text-saw-red"
            >
              {error ?? t("common.error_generic")}
            </p>
            <div className="mt-4 flex items-center justify-center gap-4">
              <button
                type="button"
                onClick={() => void refresh()}
                className="text-small text-saw-grey-700 underline underline-offset-2"
              >
                {t("common.confirm")}
              </button>
              <button
                type="button"
                onClick={() => openReport(error ?? undefined)}
                className="text-small text-saw-grey-700 underline underline-offset-2"
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
      <Onboarding
        onCompleted={() => {
          void refreshOnboarding();
        }}
      />
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
          <main className="min-h-full bg-saw-grey-50 flex items-center justify-center px-6 py-12">
            <div className="max-w-md text-center">
              <p
                role="alert"
                className="rounded-card bg-saw-white border border-saw-grey-200 px-4 py-3 text-body text-saw-red"
                data-testid="render-error-message"
              >
                {errorMessage}
              </p>
              <div className="mt-4 flex items-center justify-center gap-4">
                <button
                  type="button"
                  onClick={clear}
                  className="text-small text-saw-grey-700 underline underline-offset-2"
                >
                  {t("errordialog.dismiss")}
                </button>
                <button
                  type="button"
                  onClick={() => openReport(errorMessage)}
                  className="text-small text-saw-grey-700 underline underline-offset-2"
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
          onNavigate={(target) => setRoute(target)}
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
  if (route === "accounts") {
    return (
      <Accounts
        onClose={() => setRoute("home")}
        onOpenProfiles={() => setRoute("profiles")}
      />
    );
  }
  if (route === "profiles") {
    return <Profiles onClose={() => setRoute("accounts")} />;
  }
  if (route === "dashboard") {
    return (
      <Dashboard
        onClose={() => setRoute("home")}
        onOpenAccounts={() => setRoute("accounts")}
        onOpenReport={onOpenReport}
      />
    );
  }
  if (route === "findings") {
    return (
      <Dashboard
        onClose={() => setRoute("home")}
        onOpenAccounts={() => setRoute("accounts")}
        onOpenReport={onOpenReport}
        initialTab="findings"
      />
    );
  }
  return (
    <Home
      onOpenSettings={() => setRoute("settings")}
      onOpenAccounts={() => setRoute("accounts")}
      onOpenDashboard={() => setRoute("dashboard")}
    />
  );
}
