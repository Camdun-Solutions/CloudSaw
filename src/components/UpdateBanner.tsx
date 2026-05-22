// Update banner — Contract 16C (notify-only auto-updater).
//
// On mount the component runs `check()` from the Tauri updater
// plugin. The plugin fetches the configured `latest.json` manifest,
// verifies its Ed25519 signature against the public key in
// `tauri.conf.json`, and returns either an update descriptor or
// null. If null we render nothing — the app proceeds normally. If
// an update is available we surface a small banner with an
// "Install update" button; clicking it calls
// `downloadAndInstall()`, which the plugin runs only after the
// user's explicit action (notify-only — never silently applied).
//
// An update whose signature does not verify is rejected by the
// underlying plugin before this component ever sees it (the
// `check()` call returns null or errors with a verification
// failure code). The banner stays hidden in that case.

import { useEffect, useState } from "react";

import { useT } from "@/hooks/useT";

type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "none" }
  | { kind: "available"; version: string; date: string | null; notes: string | null }
  | { kind: "downloading"; version: string }
  | { kind: "ready"; version: string }
  | { kind: "error"; message: string };

export default function UpdateBanner() {
  const t = useT();
  const [state, setState] = useState<UpdateState>({ kind: "idle" });

  useEffect(() => {
    let cancelled = false;
    const check = async () => {
      setState({ kind: "checking" });
      try {
        // Dynamically import the plugin so the browser preview (which
        // can't reach the Tauri runtime) doesn't crash the React
        // render tree on the import side.
        const { check: doCheck } = await import("@tauri-apps/plugin-updater");
        const update = await doCheck();
        if (cancelled) return;
        if (!update) {
          setState({ kind: "none" });
          return;
        }
        setState({
          kind: "available",
          version: update.version,
          date: update.date ?? null,
          notes: update.body ?? null,
        });
      } catch (e) {
        if (cancelled) return;
        const msg = e instanceof Error ? e.message : "Update check failed.";
        setState({ kind: "error", message: msg });
      }
    };
    void check();
    return () => {
      cancelled = true;
    };
  }, []);

  async function install() {
    if (state.kind !== "available") return;
    setState({ kind: "downloading", version: state.version });
    try {
      const { check: doCheck } = await import("@tauri-apps/plugin-updater");
      const update = await doCheck();
      if (!update) {
        setState({ kind: "none" });
        return;
      }
      // The download-and-install method handles streaming, signature
      // verification, and applying the update. It runs ONLY because
      // the user clicked the button — notify-only is the rule.
      await update.downloadAndInstall();
      setState({ kind: "ready", version: state.version });
    } catch (e) {
      const msg = e instanceof Error ? e.message : "Update install failed.";
      setState({ kind: "error", message: msg });
    }
  }

  if (state.kind === "idle" || state.kind === "checking" || state.kind === "none") {
    return null;
  }
  if (state.kind === "error") {
    // Errors include the signature-rejection path. We render a
    // dismissable strip rather than blocking the UI.
    return (
      <div
        role="status"
        className="bg-saw-grey-100 text-saw-grey-700 text-small px-4 py-1 border-b border-saw-grey-200"
        data-testid="update-banner-error"
      >
        {t("updater.check_failed")}: {state.message}
      </div>
    );
  }

  return (
    <div
      role="status"
      className="bg-saw-grey-100 text-saw-grey-900 text-small px-4 py-2 border-b border-saw-grey-200 flex items-center justify-between gap-3"
      data-testid="update-banner"
    >
      <div>
        <span className="font-medium">{t("updater.available_title")}</span>{" "}
        <span className="text-saw-grey-700">v{state.version}</span>
        {state.kind === "available" && state.date ? (
          <span className="text-saw-grey-500"> · {state.date}</span>
        ) : null}
      </div>
      <div className="flex items-center gap-2">
        {state.kind === "downloading" ? (
          <span className="text-saw-grey-700">{t("updater.installing")}</span>
        ) : state.kind === "ready" ? (
          <span className="text-saw-grey-700">{t("updater.installed")}</span>
        ) : (
          <button
            type="button"
            onClick={() => void install()}
            className="rounded-card bg-saw-red text-saw-white px-3 py-1 text-xs font-medium"
            data-testid="update-banner-install"
          >
            {t("updater.install_cta")}
          </button>
        )}
      </div>
    </div>
  );
}
