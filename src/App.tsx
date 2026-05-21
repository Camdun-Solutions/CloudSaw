import { useState } from "react";

import { useT } from "@/hooks/useT";
import Accounts from "@/routes/Accounts";
import FirstRunSetup from "@/routes/FirstRunSetup";
import Home from "@/routes/Home";
import Profiles from "@/routes/Profiles";
import Settings from "@/routes/Settings";
import UnlockScreen from "@/routes/UnlockScreen";
import { useLock } from "@/stores/lock";

type Route = "home" | "accounts" | "profiles" | "settings";

export default function App() {
  const t = useT();
  const { status, state, error, refresh } = useLock();
  const [route, setRoute] = useState<Route>("home");

  if (status === "loading") {
    return (
      <main className="min-h-full bg-saw-grey-50 flex items-center justify-center">
        <p className="text-body text-saw-grey-600">{t("common.loading")}</p>
      </main>
    );
  }

  if (status === "error" || !state) {
    return (
      <main className="min-h-full bg-saw-grey-50 flex items-center justify-center px-6 py-12">
        <div className="max-w-md text-center">
          <p
            role="alert"
            className="rounded-card bg-saw-white border border-saw-grey-200 px-4 py-3 text-body text-saw-red"
          >
            {error ?? t("common.error_generic")}
          </p>
          <button
            type="button"
            onClick={() => void refresh()}
            className="mt-4 text-small text-saw-grey-700 underline underline-offset-2"
          >
            {t("common.confirm")}
          </button>
        </div>
      </main>
    );
  }

  if (state.first_run) return <FirstRunSetup />;
  if (state.locked) return <UnlockScreen />;

  if (route === "settings") {
    return <Settings onClose={() => setRoute("home")} />;
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
  return (
    <Home
      onOpenSettings={() => setRoute("settings")}
      onOpenAccounts={() => setRoute("accounts")}
    />
  );
}
