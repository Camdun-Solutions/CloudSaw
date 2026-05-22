import { useState } from "react";

import { ErrorBoundary, ErrorReportDialog } from "@/components";
import { useT } from "@/hooks/useT";
import Accounts from "@/routes/Accounts";
import ActivityLog from "@/routes/ActivityLog";
import Dashboard from "@/routes/Dashboard";
import FirstRunSetup from "@/routes/FirstRunSetup";
import Home from "@/routes/Home";
import Profiles from "@/routes/Profiles";
import ScheduledScans from "@/routes/ScheduledScans";
import Settings from "@/routes/Settings";
import UnlockScreen from "@/routes/UnlockScreen";
import { useLock } from "@/stores/lock";

type Route =
  | "home"
  | "accounts"
  | "profiles"
  | "settings"
  | "schedules"
  | "activitylog"
  | "dashboard";

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

  function openReport(notes?: string) {
    setErrorDialogNotes(notes);
    setErrorDialogOpen(true);
  }

  if (status === "loading") {
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

  if (state.first_run) return <FirstRunSetup />;
  if (state.locked) return <UnlockScreen />;

  return (
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
      <AppShell
        route={route}
        setRoute={setRoute}
        onOpenReport={openReport}
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
    </ErrorBoundary>
  );
}

function AppShell({
  route,
  setRoute,
  onOpenReport,
}: {
  route: Route;
  setRoute: (r: Route) => void;
  onOpenReport: (notes?: string) => void;
}) {
  if (route === "settings") {
    return (
      <Settings
        onClose={() => setRoute("home")}
        onOpenSchedules={() => setRoute("schedules")}
        onOpenActivityLog={() => setRoute("activitylog")}
      />
    );
  }
  if (route === "schedules") {
    return <ScheduledScans onBack={() => setRoute("settings")} />;
  }
  if (route === "activitylog") {
    return <ActivityLog onBack={() => setRoute("settings")} />;
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
  return (
    <Home
      onOpenSettings={() => setRoute("settings")}
      onOpenAccounts={() => setRoute("accounts")}
      onOpenDashboard={() => setRoute("dashboard")}
    />
  );
}
