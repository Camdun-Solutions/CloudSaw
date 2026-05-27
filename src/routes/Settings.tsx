// Settings — lock period, biometric toggle, change-password.
//
// This is the only post-unlock screen Contract 02 owns. Later contracts will
// build out the full settings surface; for now Settings is a single panel
// dedicated to app-lock configuration, reachable from the main header.

import { useCallback, useEffect, useState } from "react";

import { Button, Modal, PasswordField, Select, Switch } from "@/components";
import { useT } from "@/hooks/useT";
import { useIpcError } from "@/hooks/useIpcError";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import Accounts from "@/routes/Accounts";

import {
  ipc,
  type AiProvider,
  type AiSettings as AiSettingsT,
  type BusinessContext,
  type EnvironmentType,
  type GithubSettings,
  type LockPeriod,
  type LockSettings,
  type PanicWipeResult,
  type ReportSettings as ReportSettingsT,
  type RetentionPeriod,
  type RetentionSettings,
  type RiskTolerance,
  type TeamSize,
} from "@/lib/ipc";
import { useLock } from "@/stores/lock";

type PeriodChoice = "immediate" | "1d" | "7d" | "30d" | "never";

const PERIOD_TO_CHOICE = (p: LockPeriod): PeriodChoice => {
  if (p.kind === "immediate") return "immediate";
  if (p.kind === "never") return "never";
  switch (p.seconds) {
    case 86400:
      return "1d";
    case 604800:
      return "7d";
    case 2592000:
      return "30d";
    default:
      return "7d";
  }
};

const CHOICE_TO_PERIOD = (c: PeriodChoice): LockPeriod => {
  switch (c) {
    case "immediate":
      return { kind: "immediate" };
    case "1d":
      return { kind: "after", seconds: 86400 };
    case "7d":
      return { kind: "after", seconds: 604800 };
    case "30d":
      return { kind: "after", seconds: 2592000 };
    case "never":
      return { kind: "never" };
  }
};

type Props = {
  onClose: () => void;
  onOpenSchedules: () => void;
  onOpenActivityLog: () => void;
  /** Open the custom-report builder (Contract 15B). */
  onOpenCustomReport?: () => void;
  /** Re-enter the onboarding wizard at a specific step. Settings uses
   * this to expose "Add another AWS account" and "Re-run the full
   * wizard" actions (Contract 14 §Expected Output). */
  onRerunOnboarding?: (startAt: "aws_account" | "language") => void;
  /** Opens the standalone Profiles diagnostic view (~/.aws/config
   * reader). Wired by App.tsx to `setRoute("profiles")`. PR #46
   * moves Accounts into Settings — the embedded Accounts panel's
   * "Open profiles" button forwards here. */
  onOpenProfiles: () => void;
};

export default function Settings({
  onClose,
  onOpenSchedules,
  onOpenActivityLog,
  onOpenCustomReport,
  onRerunOnboarding,
  onOpenProfiles,
}: Props) {
  const t = useT();
  const formatError = useIpcError();
  const { state, refresh } = useLock();

  const [period, setPeriod] = useState<PeriodChoice>("7d");
  const [biometric, setBiometric] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [savedFlash, setSavedFlash] = useState(false);
  const [changeOpen, setChangeOpen] = useState(false);

  // Hydrate from store on mount / when settings change underneath us.
  useEffect(() => {
    if (!state) return;
    setPeriod(PERIOD_TO_CHOICE(state.settings.lock_period));
    setBiometric(state.settings.biometric_enabled);
  }, [state]);

  const biometricSupported = state?.biometric_availability === "Available";
  const biometricUnconfigured =
    state?.biometric_availability === "Unconfigured";

  async function save() {
    setSaving(true);
    setSaveError(null);
    setSavedFlash(false);
    try {
      const next: LockSettings = {
        lock_period: CHOICE_TO_PERIOD(period),
        biometric_enabled: biometric,
      };
      await ipc.applockSetSettings(next);
      await refresh();
      setSavedFlash(true);
      window.setTimeout(() => setSavedFlash(false), 2000);
    } catch (err) {
      setSaveError(formatError(err));
    } finally {
      setSaving(false);
    }
  }

  async function onLock() {
    try {
      await ipc.applockLock();
      await refresh();
      onClose();
    } catch (err) {
      setSaveError(formatError(err));
    }
  }

  if (!state) return null;

  const periodOptions: { value: PeriodChoice; label: string }[] = [
    { value: "immediate", label: t("applock.settings.period.immediate") },
    { value: "1d", label: t("applock.settings.period.1d") },
    { value: "7d", label: t("applock.settings.period.7d") },
    { value: "30d", label: t("applock.settings.period.30d") },
    { value: "never", label: t("applock.settings.period.never") },
  ];

  return (
    <main className="min-h-full bg-saw-grey-50 px-8 py-10">
      <header className="mb-6 flex items-center justify-between">
        <div>
          <h1 className="text-h1 font-semibold text-saw-grey-900">
            {t("nav.settings")}
          </h1>
          <p className="mt-1 text-small text-saw-grey-600">
            {t("applock.settings.subtitle")}
          </p>
        </div>
        <Button variant="ghost" onClick={onClose}>
          {t("common.close")}
        </Button>
      </header>

      <section className="max-w-2xl rounded-card bg-saw-white border border-saw-grey-200 p-6">
        <h2 className="text-h3 font-semibold text-saw-grey-900">
          {t("applock.settings.section.app_lock")}
        </h2>
        <p className="mt-1 text-small text-saw-grey-600">
          {t("applock.disclosure")}
        </p>

        <div className="mt-6 flex flex-col gap-6">
          <Select<PeriodChoice>
            label={t("applock.settings.period.label")}
            value={period}
            options={periodOptions}
            onChange={setPeriod}
            description={
              period === "never"
                ? t("applock.settings.period.never_warning")
                : undefined
            }
            data-testid="settings-period"
          />

          <Switch
            label={t("applock.settings.biometric.label")}
            description={t("applock.settings.biometric.description")}
            checked={biometric && biometricSupported}
            onChange={setBiometric}
            disabled={!biometricSupported}
            disabledReason={
              biometricUnconfigured
                ? t("applock.settings.biometric.unconfigured")
                : t("applock.settings.biometric.unavailable")
            }
          />

          {saveError ? (
            <p
              role="alert"
              className="rounded-card bg-saw-grey-100 px-3 py-2 text-small text-saw-red"
            >
              {saveError}
            </p>
          ) : null}
          {savedFlash ? (
            <p
              role="status"
              className="rounded-card bg-saw-grey-100 px-3 py-2 text-small text-saw-grey-700"
            >
              {t("applock.settings.saved")}
            </p>
          ) : null}

          <div className="flex flex-wrap gap-3">
            <Button
              variant="primary"
              onClick={save}
              disabled={saving}
              data-testid="settings-save"
            >
              {saving ? t("common.loading") : t("common.save")}
            </Button>
            <Button
              variant="secondary"
              onClick={() => setChangeOpen(true)}
              data-testid="settings-change-password"
            >
              {t("applock.settings.change_password")}
            </Button>
            <Button
              variant="ghost"
              onClick={onLock}
              data-testid="settings-lock-now"
            >
              {t("applock.settings.lock_now")}
            </Button>
          </div>
        </div>
      </section>

      {/* PR #46 — Accounts moved into Settings. The Accounts
          component renders in `embedded` mode here: its outer
          <main> + page-level header are skipped, but every modal
          (add / edit / remove / connect-scanner-role / scan) and
          interaction works identically to the legacy standalone
          route. PR #47 (Settings left menu) will move this into
          its own panel rather than a stacked section. */}
      <section
        className="mt-6 max-w-4xl rounded-card bg-saw-white border border-saw-grey-200 p-6"
        data-testid="settings-section-accounts"
      >
        <h2 className="text-h3 font-semibold text-saw-grey-900">
          {t("accounts.title")}
        </h2>
        <p className="mt-1 text-small text-saw-grey-600">
          {t("accounts.subtitle")}
        </p>
        <div className="mt-4">
          <Accounts embedded onOpenProfiles={onOpenProfiles} />
        </div>
      </section>

      <section
        className="mt-6 max-w-2xl rounded-card bg-saw-white border border-saw-grey-200 p-6"
        data-testid="settings-section-schedules"
      >
        <h2 className="text-h3 font-semibold text-saw-grey-900">
          {t("schedules.section_title")}
        </h2>
        <p className="mt-1 text-small text-saw-grey-600">
          {t("schedules.section_subtitle")}
        </p>
        <div className="mt-4">
          <Button
            variant="secondary"
            onClick={onOpenSchedules}
            data-testid="settings-open-schedules"
          >
            {t("schedules.section_cta")}
          </Button>
        </div>
      </section>

      <ActivityLogSection onOpen={onOpenActivityLog} />
      <OnboardingSection onRerun={onRerunOnboarding} />
      <ReportSection onOpenCustomReport={onOpenCustomReport} />
      <RetentionSection />
      <UpdatesSection />
      <GithubSection />
      <AiSection />
      <PanicSection />

      <ChangePasswordDialog
        open={changeOpen}
        onClose={() => setChangeOpen(false)}
        onChanged={async () => {
          setChangeOpen(false);
          await refresh();
        }}
      />
    </main>
  );
}

// --- Updates section ----------------------------------------------------
//
// Two pieces of state the user controls:
//   1. Whether CloudSaw auto-checks on launch (persisted via
//      `updatePrefs` in localStorage; default ON). The UpdateBanner
//      reads the same flag and skips its on-mount check when off.
//   2. A manual "Check for updates" button that runs the same
//      `check()` from the Tauri updater plugin regardless of the
//      auto-check toggle, surfacing the available version + a link
//      to the GitHub release notes.

type UpdateCheckResult =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "up_to_date"; at: string }
  | { kind: "available"; version: string; at: string }
  | { kind: "error"; message: string; at: string };

function UpdatesSection() {
  const t = useT();
  const [autoCheck, setAutoCheck] = useState<boolean>(true);
  const [installedVersion, setInstalledVersion] = useState<string | null>(null);
  const [result, setResult] = useState<UpdateCheckResult>({ kind: "idle" });

  useEffect(() => {
    let cancelled = false;
    void import("@/lib/updatePrefs").then(({ getAutoCheckEnabled }) => {
      if (cancelled) return;
      setAutoCheck(getAutoCheckEnabled());
    });
    void import("@tauri-apps/api/app")
      .then(({ getVersion }) => getVersion())
      .then((v) => {
        if (cancelled) return;
        setInstalledVersion(v);
      })
      .catch(() => {
        // In a non-Tauri context (e.g. the browser dev preview) the
        // import will reject. Leaving installedVersion null causes the
        // line to render an em-dash placeholder.
      });
    return () => {
      cancelled = true;
    };
  }, []);

  async function persistAutoCheck(next: boolean) {
    setAutoCheck(next);
    const { setAutoCheckEnabled } = await import("@/lib/updatePrefs");
    setAutoCheckEnabled(next);
  }

  async function manualCheck() {
    setResult({ kind: "checking" });
    const at = new Date().toISOString();
    try {
      const { check: doCheck } = await import("@tauri-apps/plugin-updater");
      const update = await doCheck();
      if (!update) {
        setResult({ kind: "up_to_date", at });
        return;
      }
      setResult({ kind: "available", version: update.version, at });
    } catch (e) {
      const msg = e instanceof Error ? e.message : "Update check failed.";
      setResult({ kind: "error", message: msg, at });
    }
  }

  const lastCheckedLabel =
    result.kind === "idle" || result.kind === "checking"
      ? t("settings.updates.never_checked")
      : formatTimestamp(result.at);

  return (
    <section
      className="mt-6 max-w-2xl rounded-card bg-saw-white border border-saw-grey-200 p-6"
      data-testid="settings-section-updates"
      aria-labelledby="settings-updates-title"
    >
      <h2
        id="settings-updates-title"
        className="text-h3 font-semibold text-saw-grey-900"
      >
        {t("settings.section.updates_title")}
      </h2>
      <p className="mt-1 text-small text-saw-grey-600">
        {t("settings.section.updates_subtitle")}
      </p>

      <div className="mt-4">
        <Switch
          checked={autoCheck}
          onChange={(next) => void persistAutoCheck(next)}
          label={t("settings.updates.auto_toggle_label")}
          description={t("settings.updates.auto_toggle_description")}
        />
      </div>

      <hr className="my-4 border-saw-grey-100" />

      <dl className="grid grid-cols-[max-content_1fr] gap-x-3 gap-y-1 text-small">
        <dt className="text-saw-grey-500">
          {t("settings.updates.installed_version_label")}
        </dt>
        <dd
          className="font-mono text-saw-grey-900"
          data-testid="settings-updates-installed-version"
        >
          {installedVersion ?? "—"}
        </dd>
        <dt className="text-saw-grey-500">
          {t("settings.updates.last_checked_label")}
        </dt>
        <dd className="text-saw-grey-900" data-testid="settings-updates-last-checked">
          {lastCheckedLabel}
        </dd>
      </dl>

      <div className="mt-4">
        <Button
          variant="secondary"
          onClick={() => void manualCheck()}
          disabled={result.kind === "checking"}
          data-testid="settings-updates-check"
        >
          {result.kind === "checking"
            ? t("settings.updates.checking")
            : t("settings.updates.check_cta")}
        </Button>
      </div>

      {result.kind === "up_to_date" ? (
        <p
          role="status"
          className="mt-4 rounded-card bg-saw-grey-50 px-3 py-2 text-small text-saw-grey-800"
          data-testid="settings-updates-result-up-to-date"
        >
          {t("settings.updates.up_to_date")}
        </p>
      ) : null}

      {result.kind === "available" ? (
        <div
          role="status"
          className="mt-4 rounded-card border border-saw-grey-200 bg-saw-grey-50 px-3 py-3 text-small text-saw-grey-800"
          data-testid="settings-updates-result-available"
        >
          <p className="font-semibold text-saw-grey-900">
            {t("settings.updates.available_title")}
          </p>
          <p className="mt-1">
            {t("settings.updates.available_body").replace(
              "{version}",
              result.version,
            )}
          </p>
          <p className="mt-2">
            <a
              href={`https://github.com/Camdun-Solutions/CloudSaw/releases/tag/${encodeURIComponent(result.version)}`}
              target="_blank"
              rel="noopener noreferrer"
              className="underline underline-offset-2"
              data-testid="settings-updates-release-notes-link"
            >
              {t("settings.updates.release_notes_link")}
            </a>
          </p>
        </div>
      ) : null}

      {result.kind === "error" ? (
        <div
          role="alert"
          className="mt-4 rounded-card border border-saw-red/30 bg-saw-red/5 px-3 py-3 text-small text-saw-grey-900"
          data-testid="settings-updates-result-error"
        >
          <p className="font-semibold text-saw-red">
            {t("settings.updates.check_failed_title")}
          </p>
          <p className="mt-1 text-saw-grey-800">
            {t("settings.updates.check_failed_body")}
          </p>
        </div>
      ) : null}
    </section>
  );
}

function formatTimestamp(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleString();
}

// --- Contract 11 sections -----------------------------------------------

function OnboardingSection({
  onRerun,
}: {
  onRerun?: (startAt: "aws_account" | "language") => void;
}) {
  const t = useT();
  if (!onRerun) return null;
  return (
    <section
      className="mt-6 max-w-2xl rounded-card bg-saw-white border border-saw-grey-200 p-6"
      data-testid="settings-section-onboarding"
    >
      <h2 className="text-h3 font-semibold text-saw-grey-900">
        {t("settings.section.onboarding_title")}
      </h2>
      <p className="mt-1 text-small text-saw-grey-600">
        {t("settings.section.onboarding_subtitle")}
      </p>
      <div className="mt-4 flex flex-wrap gap-2">
        <Button
          variant="secondary"
          onClick={() => onRerun("aws_account")}
          data-testid="settings-onboarding-add-account"
        >
          {t("settings.section.onboarding_add_account")}
        </Button>
        <Button
          variant="ghost"
          onClick={() => onRerun("language")}
          data-testid="settings-onboarding-rerun-full"
        >
          {t("settings.section.onboarding_rerun_full")}
        </Button>
      </div>
    </section>
  );
}

function ActivityLogSection({ onOpen }: { onOpen: () => void }) {
  const t = useT();
  return (
    <section
      className="mt-6 max-w-2xl rounded-card bg-saw-white border border-saw-grey-200 p-6"
      data-testid="settings-section-activitylog"
    >
      <h2 className="text-h3 font-semibold text-saw-grey-900">
        {t("eventlog.section_title")}
      </h2>
      <p className="mt-1 text-small text-saw-grey-600">
        {t("eventlog.section_subtitle")}
      </p>
      <div className="mt-4">
        <Button
          variant="secondary"
          onClick={onOpen}
          data-testid="settings-open-activitylog"
        >
          {t("eventlog.section_cta")}
        </Button>
      </div>
    </section>
  );
}

type RetentionChoice = "30d" | "60d" | "90d" | "180d" | "365d" | "never";

function periodToChoice(p: RetentionPeriod): RetentionChoice {
  if (p.kind === "never") return "never";
  switch (p.days) {
    case 30: return "30d";
    case 60: return "60d";
    case 90: return "90d";
    case 180: return "180d";
    case 365: return "365d";
    default: return "90d";
  }
}

function choiceToPeriod(c: RetentionChoice): RetentionPeriod {
  switch (c) {
    case "never": return { kind: "never" };
    case "30d": return { kind: "days", days: 30 };
    case "60d": return { kind: "days", days: 60 };
    case "90d": return { kind: "days", days: 90 };
    case "180d": return { kind: "days", days: 180 };
    case "365d": return { kind: "days", days: 365 };
  }
}

function RetentionSection() {
  const t = useT();
  const formatError = useIpcError();
  const [settings, setSettings] = useState<RetentionSettings | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [toast, setToast] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      setSettings(await ipc.retentionGetSettings());
    } catch (e) {
      setErr(formatError(e));
    }
  }, [formatError]);

  useEffect(() => {
    void reload();
  }, [reload]);

  if (!settings) {
    return null;
  }

  const scanChoice = periodToChoice(settings.scan_retention);
  const eventChoice = periodToChoice(settings.eventlog_retention);

  const options: { value: RetentionChoice; label: string }[] = [
    { value: "30d", label: t("retention.period.30d") },
    { value: "60d", label: t("retention.period.60d") },
    { value: "90d", label: t("retention.period.90d") },
    { value: "180d", label: t("retention.period.180d") },
    { value: "365d", label: t("retention.period.365d") },
    { value: "never", label: t("retention.period.never") },
  ];

  async function updateScan(c: RetentionChoice) {
    setErr(null);
    try {
      await ipc.retentionSetScan(choiceToPeriod(c));
      await reload();
    } catch (e) {
      setErr(formatError(e));
    }
  }
  async function updateEventlog(c: RetentionChoice) {
    setErr(null);
    try {
      await ipc.retentionSetEventlog(choiceToPeriod(c));
      await reload();
    } catch (e) {
      setErr(formatError(e));
    }
  }
  async function runNow() {
    setBusy(true);
    setErr(null);
    setToast(null);
    try {
      const summary = await ipc.retentionRunNow();
      setToast(
        t("retention.toast")
          .replace("{scans}", String(summary.scan_dirs_removed))
          .replace("{raw}", String(summary.raw_files_removed))
          .replace("{events}", String(summary.eventlog_rows_removed)),
      );
      await reload();
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setBusy(false);
      window.setTimeout(() => setToast(null), 4000);
    }
  }

  const lastRun = settings.last_run_at
    ? t("retention.last_run").replace("{at}", new Date(settings.last_run_at).toLocaleString())
    : t("retention.never_run");

  return (
    <section
      className="mt-6 max-w-2xl rounded-card bg-saw-white border border-saw-grey-200 p-6"
      data-testid="settings-section-retention"
    >
      <h2 className="text-h3 font-semibold text-saw-grey-900">
        {t("retention.section_title")}
      </h2>
      <p className="mt-1 text-small text-saw-grey-600">
        {t("retention.section_subtitle")}
      </p>

      <div className="mt-4 flex flex-col gap-4">
        <Select<RetentionChoice>
          label={t("retention.scan.label")}
          description={t("retention.scan.hint")}
          value={scanChoice}
          options={options}
          onChange={(c) => void updateScan(c)}
          data-testid="settings-retention-scan"
        />
        <Select<RetentionChoice>
          label={t("retention.eventlog.label")}
          description={t("retention.eventlog.hint")}
          value={eventChoice}
          options={options}
          onChange={(c) => void updateEventlog(c)}
          data-testid="settings-retention-eventlog"
        />
        {(scanChoice === "never" || eventChoice === "never") ? (
          <p className="text-small text-saw-grey-600">
            {t("retention.never_storage_hint")}
          </p>
        ) : null}
        <p className="text-small text-saw-grey-500">{lastRun}</p>

        {err ? (
          <p role="alert" className="rounded-card bg-saw-grey-100 px-3 py-2 text-small text-saw-red">
            {err}
          </p>
        ) : null}
        {toast ? (
          <p
            role="status"
            className="rounded-card bg-saw-grey-100 px-3 py-2 text-small text-saw-grey-700"
            data-testid="settings-retention-toast"
          >
            {toast}
          </p>
        ) : null}

        <div>
          <Button
            variant="secondary"
            onClick={() => void runNow()}
            disabled={busy}
            data-testid="settings-retention-run"
          >
            {busy ? t("retention.run_busy") : t("retention.run_now")}
          </Button>
        </div>
      </div>
    </section>
  );
}

function PanicSection() {
  const t = useT();
  const formatError = useIpcError();
  const [open, setOpen] = useState(false);
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [result, setResult] = useState<PanicWipeResult | null>(null);

  function close() {
    setOpen(false);
    setConfirm("");
    setErr(null);
  }

  async function doPanic() {
    if (confirm !== "PANIC") {
      setErr(t("eventlog.error.confirmation_rejected"));
      return;
    }
    setBusy(true);
    setErr(null);
    try {
      const out = await ipc.systemPanicWipe(confirm);
      setResult(out);
      setOpen(false);
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setBusy(false);
      setConfirm("");
    }
  }

  async function doReboot() {
    try {
      await ipc.systemRequestReboot();
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setResult(null);
    }
  }

  return (
    <>
      <section
        className="mt-6 max-w-2xl rounded-card bg-saw-white border border-saw-red/40 p-6"
        data-testid="settings-section-panic"
      >
        <h2 className="text-h3 font-semibold text-saw-red">{t("panic.section_title")}</h2>
        <p className="mt-1 text-small text-saw-grey-700">{t("panic.section_subtitle")}</p>
        <div className="mt-4">
          <Button
            variant="primary"
            onClick={() => setOpen(true)}
            data-testid="settings-panic-cta"
          >
            {t("panic.section_cta")}
          </Button>
        </div>
      </section>

      <Modal
        open={open}
        onClose={close}
        title={t("panic.title")}
        footer={
          <>
            <Button variant="ghost" onClick={close} disabled={busy}>
              {t("panic.cancel")}
            </Button>
            <Button
              variant="primary"
              onClick={() => void doPanic()}
              disabled={busy || confirm !== "PANIC"}
              data-testid="panic-confirm"
            >
              {busy ? t("panic.busy") : t("panic.confirm_cta")}
            </Button>
          </>
        }
      >
        <div className="flex flex-col gap-3">
          <p>{t("panic.explainer")}</p>
          <p className="text-small text-saw-red">{t("panic.warning")}</p>
          <label className="flex flex-col gap-1 text-small text-saw-grey-700">
            <span>{t("panic.confirm_label")}</span>
            <input
              type="text"
              value={confirm}
              onChange={(e) => setConfirm(e.target.value)}
              placeholder={t("panic.confirm_placeholder")}
              autoFocus
              className="rounded-card border border-saw-grey-200 bg-saw-white px-3 py-1.5 text-body text-saw-grey-900"
              data-testid="panic-confirm-input"
            />
          </label>
          {err ? (
            <p role="alert" className="rounded-card bg-saw-grey-100 px-3 py-2 text-small text-saw-red">
              {err}
            </p>
          ) : null}
        </div>
      </Modal>

      {result ? (
        <Modal
          open={!!result}
          onClose={() => setResult(null)}
          title={t("panic.success.title")}
          footer={
            <>
              <Button
                variant="ghost"
                onClick={() => setResult(null)}
                data-testid="panic-later"
              >
                {t("panic.success.later")}
              </Button>
              <Button
                variant="primary"
                onClick={() => void doReboot()}
                data-testid="panic-reboot-now"
              >
                {t("panic.success.reboot_now")}
              </Button>
            </>
          }
        >
          <div className="flex flex-col gap-3">
            <p>
              {t("panic.success.body")
                .replace("{scans}", String(result.scan_dirs_removed))
                .replace("{tf}", String(result.tf_workdirs_removed))
                .replace("{logs}", String(result.log_files_removed))
                .replace("{dbs}", String(result.db_files_removed))
                .replace("{keychain}", String(result.keychain.removed))
                .replace(
                  "{staged}",
                  result.self_delete_staged
                    ? t("panic.success.staged_yes")
                    : t("panic.success.staged_no"),
                )}
            </p>
            <p className="text-small text-saw-grey-700">
              {t("panic.success.reboot_question")}
            </p>
          </div>
        </Modal>
      ) : null}
    </>
  );
}

const MIN_PASSWORD_LEN = 8;

function ChangePasswordDialog({
  open,
  onClose,
  onChanged,
}: {
  open: boolean;
  onClose: () => void;
  onChanged: () => Promise<void>;
}) {
  const t = useT();
  const formatError = useIpcError();
  const [oldPw, setOldPw] = useState("");
  const [newPw, setNewPw] = useState("");
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) {
      setOldPw("");
      setNewPw("");
      setConfirm("");
      setError(null);
      setBusy(false);
    }
  }, [open]);

  const tooShort = newPw.length > 0 && newPw.length < MIN_PASSWORD_LEN;
  const mismatch = newPw.length > 0 && confirm.length > 0 && newPw !== confirm;
  const canSubmit =
    !busy &&
    oldPw.length > 0 &&
    newPw.length >= MIN_PASSWORD_LEN &&
    newPw === confirm;

  async function onSubmit() {
    if (!canSubmit) return;
    setBusy(true);
    setError(null);
    try {
      await ipc.applockChangePassword(oldPw, newPw);
      setOldPw("");
      setNewPw("");
      setConfirm("");
      await onChanged();
    } catch (err) {
      setError(formatError(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={t("applock.settings.change_password")}
      footer={
        <>
          <Button variant="ghost" onClick={onClose} disabled={busy}>
            {t("common.cancel")}
          </Button>
          <Button
            variant="primary"
            onClick={onSubmit}
            disabled={!canSubmit}
            data-testid="change-password-submit"
          >
            {busy ? t("applock.recovery.busy") : t("applock.settings.change_password")}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-4">
        <PasswordField
          label={t("applock.field.old_password")}
          value={oldPw}
          onChange={(e) => setOldPw(e.target.value)}
          autoComplete="current-password"
          showLabel={t("applock.field.show")}
          hideLabel={t("applock.field.hide")}
        />
        <PasswordField
          label={t("applock.field.new_password")}
          value={newPw}
          onChange={(e) => setNewPw(e.target.value)}
          autoComplete="new-password"
          hint={t("applock.setup.password_hint").replace(
            "{min}",
            String(MIN_PASSWORD_LEN),
          )}
          error={tooShort ? t("applock.error.too_short") : null}
          showLabel={t("applock.field.show")}
          hideLabel={t("applock.field.hide")}
        />
        <PasswordField
          label={t("applock.field.confirm_password")}
          value={confirm}
          onChange={(e) => setConfirm(e.target.value)}
          autoComplete="new-password"
          error={mismatch ? t("applock.error.mismatch") : null}
          showLabel={t("applock.field.show")}
          hideLabel={t("applock.field.hide")}
        />
        {error ? (
          <p
            role="alert"
            className="rounded-card bg-saw-grey-100 px-3 py-2 text-small text-saw-red"
          >
            {error}
          </p>
        ) : null}
      </div>
    </Modal>
  );
}

// --- GitHub integration (Contract 12) -----------------------------------

function GithubSection() {
  const t = useT();
  const formatError = useIpcError();
  const [settings, setSettings] = useState<GithubSettings | null>(null);
  const [tokenInput, setTokenInput] = useState("");
  const [tokenBusy, setTokenBusy] = useState(false);
  const [tokenSaved, setTokenSaved] = useState(false);
  const [repoInput, setRepoInput] = useState("");
  const [repoBusy, setRepoBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      const s = await ipc.githubGetSettings();
      setSettings(s);
      setRepoInput(s.findings_repo ? `${s.findings_repo.owner}/${s.findings_repo.name}` : "");
    } catch (e) {
      setErr(formatError(e));
    }
  }, [formatError]);

  useEffect(() => {
    void reload();
  }, [reload]);

  async function saveToken() {
    setErr(null);
    setTokenBusy(true);
    setTokenSaved(false);
    try {
      await ipc.githubSetToken(tokenInput);
      setTokenInput("");
      setTokenSaved(true);
      window.setTimeout(() => setTokenSaved(false), 3000);
      await reload();
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setTokenBusy(false);
    }
  }
  async function clearToken() {
    setErr(null);
    try {
      await ipc.githubClearToken();
      await reload();
    } catch (e) {
      setErr(formatError(e));
    }
  }
  async function saveRepo() {
    setErr(null);
    setRepoBusy(true);
    try {
      const parts = repoInput.trim().split("/");
      if (parts.length !== 2 || !parts[0] || !parts[1]) {
        setErr(t("github.error.no_findings_repo"));
        return;
      }
      await ipc.githubSetFindingsRepo({ owner: parts[0], name: parts[1] });
      await reload();
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setRepoBusy(false);
    }
  }
  async function clearRepo() {
    setErr(null);
    try {
      await ipc.githubSetFindingsRepo(null);
      setRepoInput("");
      await reload();
    } catch (e) {
      setErr(formatError(e));
    }
  }
  async function openTokenPage() {
    try {
      const url = await ipc.githubGenerateTokenUrl();
      window.open(url, "_blank", "noopener,noreferrer");
    } catch (e) {
      setErr(formatError(e));
    }
  }

  if (!settings) return null;

  return (
    <section
      className="mt-6 max-w-2xl rounded-card bg-saw-white border border-saw-grey-200 p-6"
      data-testid="settings-section-github"
    >
      <h2 className="text-h3 font-semibold text-saw-grey-900">
        {t("github.section_title")}
      </h2>
      <p className="mt-1 text-small text-saw-grey-600">
        {t("github.section_subtitle")}
      </p>

      <div className="mt-4 flex flex-col gap-4">
        <p
          className="text-small text-saw-grey-700"
          data-testid="settings-github-token-status"
        >
          {settings.token.configured
            ? t("github.token.configured")
            : t("github.token.not_configured")}
        </p>
        <label className="flex flex-col gap-1 text-small text-saw-grey-700">
          <span>{t("github.token.label")}</span>
          <input
            type="password"
            value={tokenInput}
            onChange={(e) => setTokenInput(e.target.value)}
            placeholder={t("github.token.placeholder")}
            autoComplete="off"
            className="rounded-card border border-saw-grey-200 bg-saw-white px-3 py-1.5 text-body text-saw-grey-900 font-mono"
            data-testid="settings-github-token-input"
          />
          <span className="text-xs text-saw-grey-500">{t("github.token.hint")}</span>
        </label>
        <div className="flex flex-wrap gap-2">
          <Button
            variant="primary"
            onClick={() => void saveToken()}
            disabled={tokenBusy || tokenInput.trim().length === 0}
            data-testid="settings-github-token-save"
          >
            {tokenBusy ? t("github.token.saving") : t("github.token.save")}
          </Button>
          <Button
            variant="ghost"
            onClick={() => void openTokenPage()}
            data-testid="settings-github-generate"
          >
            {t("github.token.generate_cta")}
          </Button>
          {settings.token.configured ? (
            <Button
              variant="ghost"
              onClick={() => void clearToken()}
              data-testid="settings-github-token-clear"
            >
              {t("github.token.clear")}
            </Button>
          ) : null}
        </div>
        {tokenSaved ? (
          <p
            role="status"
            className="rounded-card bg-saw-grey-100 px-3 py-2 text-small text-saw-grey-700"
            data-testid="settings-github-token-saved"
          >
            {t("github.token.configured")}
          </p>
        ) : null}

        <hr className="border-saw-grey-100" />

        <label className="flex flex-col gap-1 text-small text-saw-grey-700">
          <span>{t("github.findings_repo.label")}</span>
          <input
            type="text"
            value={repoInput}
            onChange={(e) => setRepoInput(e.target.value)}
            placeholder={t("github.findings_repo.placeholder")}
            className="rounded-card border border-saw-grey-200 bg-saw-white px-3 py-1.5 text-body text-saw-grey-900 font-mono"
            data-testid="settings-github-repo-input"
          />
          <span className="text-xs text-saw-grey-500">{t("github.findings_repo.hint")}</span>
        </label>
        <div className="flex flex-wrap gap-2">
          <Button
            variant="primary"
            onClick={() => void saveRepo()}
            disabled={repoBusy || repoInput.trim().length === 0}
            data-testid="settings-github-repo-save"
          >
            {t("github.findings_repo.save")}
          </Button>
          {settings.findings_repo ? (
            <Button
              variant="ghost"
              onClick={() => void clearRepo()}
              data-testid="settings-github-repo-clear"
            >
              {t("github.findings_repo.clear")}
            </Button>
          ) : null}
        </div>
        {!settings.findings_repo ? (
          <p className="text-small text-saw-grey-500" data-testid="settings-github-repo-none">
            {t("github.findings_repo.none")}
          </p>
        ) : null}

        <hr className="border-saw-grey-100" />

        <div className="text-small text-saw-grey-700">
          <div className="font-medium">{t("github.error_repo.label")}</div>
          <div className="font-mono text-saw-grey-900">
            {settings.error_report_repo.owner}/{settings.error_report_repo.name}
          </div>
          <div className="text-xs text-saw-grey-500 mt-1">
            {t("github.error_repo.hint")}
          </div>
        </div>
        <div className="text-small text-saw-grey-700">
          <div className="font-medium">{t("github.security_contact.label")}</div>
          <div
            className="font-mono text-saw-grey-900"
            data-testid="settings-github-security-contact"
          >
            {settings.security_contact}
          </div>
          <div className="text-xs text-saw-grey-500 mt-1">
            {t("github.security_contact.hint")}
          </div>
        </div>

        {err ? (
          <p role="alert" className="rounded-card bg-saw-grey-100 px-3 py-2 text-small text-saw-red">
            {err}
          </p>
        ) : null}
      </div>
    </section>
  );
}

// --- AI Suggestion Layer (Contract 13) ----------------------------------

function AiSection() {
  const t = useT();
  const formatError = useIpcError();
  const [settings, setSettings] = useState<AiSettingsT | null>(null);
  const [provider, setProvider] = useState<AiProvider | "">("");
  const [keyInput, setKeyInput] = useState("");
  const [keyBusy, setKeyBusy] = useState(false);
  const [keySaved, setKeySaved] = useState(false);
  const [context, setContext] = useState<BusinessContext | null>(null);
  const [ctxSaved, setCtxSaved] = useState(false);
  const [complianceInput, setComplianceInput] = useState("");
  const [err, setErr] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      const s = await ipc.aiGetSettings();
      setSettings(s);
      setProvider(s.provider ?? "");
      setContext(s.context);
      setComplianceInput(s.context.compliance.join(", "));
    } catch (e) {
      setErr(formatError(e));
    }
  }, [formatError]);

  useEffect(() => {
    void reload();
  }, [reload]);

  if (!settings || !context) return null;

  async function saveProvider() {
    setErr(null);
    try {
      await ipc.aiSetProvider(provider === "" ? null : provider);
      await reload();
    } catch (e) {
      setErr(formatError(e));
    }
  }
  async function saveKey() {
    setErr(null);
    if (provider === "") {
      setErr(t("ai.error.no_provider"));
      return;
    }
    setKeyBusy(true);
    setKeySaved(false);
    try {
      await ipc.aiSetProviderKey(provider, keyInput);
      setKeyInput("");
      setKeySaved(true);
      window.setTimeout(() => setKeySaved(false), 3000);
      await reload();
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setKeyBusy(false);
    }
  }
  async function clearKey() {
    setErr(null);
    if (provider === "") return;
    try {
      await ipc.aiClearProviderKey(provider);
      await reload();
    } catch (e) {
      setErr(formatError(e));
    }
  }
  async function saveContext() {
    setErr(null);
    if (!context) return;
    setCtxSaved(false);
    try {
      const compliance = complianceInput
        .split(",")
        .map((s) => s.trim())
        .filter((s) => s.length > 0);
      const next: BusinessContext = { ...context, compliance };
      await ipc.aiSetBusinessContext(next);
      setCtxSaved(true);
      window.setTimeout(() => setCtxSaved(false), 3000);
      await reload();
    } catch (e) {
      setErr(formatError(e));
    }
  }

  const envOptions: { value: EnvironmentType; label: string }[] = [
    { value: "unspecified", label: t("ai.context.env.unspecified") },
    { value: "production", label: t("ai.context.env.production") },
    { value: "dev_test", label: t("ai.context.env.dev_test") },
    { value: "mixed", label: t("ai.context.env.mixed") },
  ];
  const riskOptions: { value: RiskTolerance; label: string }[] = [
    { value: "unspecified", label: t("ai.context.risk.unspecified") },
    { value: "low", label: t("ai.context.risk.low") },
    { value: "medium", label: t("ai.context.risk.medium") },
    { value: "high", label: t("ai.context.risk.high") },
  ];
  const teamOptions: { value: TeamSize; label: string }[] = [
    { value: "unspecified", label: t("ai.context.team.unspecified") },
    { value: "solo", label: t("ai.context.team.solo") },
    { value: "small", label: t("ai.context.team.small") },
    { value: "medium", label: t("ai.context.team.medium") },
    { value: "large", label: t("ai.context.team.large") },
  ];

  return (
    <section
      className="mt-6 max-w-2xl rounded-card bg-saw-white border border-saw-grey-200 p-6"
      data-testid="settings-section-ai"
    >
      <h2 className="text-h3 font-semibold text-saw-grey-900">
        {t("ai.section_title")}
      </h2>
      <p className="mt-1 text-small text-saw-grey-600">
        {t("ai.section_subtitle")}
      </p>

      {!settings.key_connected ? (
        <div
          className="mt-4 rounded-card border border-saw-grey-200 bg-saw-grey-50 p-3 text-small"
          data-testid="ai-dormant-note"
        >
          <div className="font-medium text-saw-grey-900">
            {t("ai.dormant.title")}
          </div>
          <div className="text-saw-grey-700 mt-1">{t("ai.dormant.body")}</div>
        </div>
      ) : null}

      <div className="mt-4 rounded-card border border-saw-red/30 bg-saw-red/5 p-3 text-small">
        <div className="font-medium text-saw-red">{t("ai.disclosure.title")}</div>
        <div className="text-saw-grey-800 mt-1">{t("ai.disclosure.body")}</div>
      </div>

      <div className="mt-4 flex flex-col gap-4">
        <label className="flex flex-col gap-1 text-small text-saw-grey-700">
          <span>{t("ai.provider.label")}</span>
          <select
            value={provider}
            onChange={(e) => setProvider(e.target.value as AiProvider | "")}
            className="rounded-card border border-saw-grey-200 bg-saw-white px-3 py-1.5 text-body text-saw-grey-900"
            data-testid="ai-provider-select"
          >
            <option value="">{t("ai.provider.none")}</option>
            <option value="anthropic">{t("ai.provider.anthropic")}</option>
            <option value="openai">{t("ai.provider.openai")}</option>
          </select>
        </label>
        <div>
          <Button
            variant="secondary"
            onClick={() => void saveProvider()}
            data-testid="ai-provider-save"
          >
            {provider === "" ? t("ai.provider.clear") : t("ai.provider.set")}
          </Button>
        </div>

        {provider !== "" ? (
          <>
            <label className="flex flex-col gap-1 text-small text-saw-grey-700">
              <span>{t("ai.key.label")}</span>
              <input
                type="password"
                value={keyInput}
                onChange={(e) => setKeyInput(e.target.value)}
                placeholder={
                  provider === "anthropic"
                    ? t("ai.key.placeholder_anthropic")
                    : t("ai.key.placeholder_openai")
                }
                autoComplete="off"
                className="rounded-card border border-saw-grey-200 bg-saw-white px-3 py-1.5 text-body text-saw-grey-900 font-mono"
                data-testid="ai-key-input"
              />
              <span className="text-xs text-saw-grey-500">{t("ai.key.hint")}</span>
            </label>
            <div className="flex flex-wrap gap-2">
              <Button
                variant="primary"
                onClick={() => void saveKey()}
                disabled={keyBusy || keyInput.trim().length === 0}
                data-testid="ai-key-save"
              >
                {keyBusy ? t("ai.key.saving") : t("ai.key.save")}
              </Button>
              {settings.key_connected ? (
                <Button
                  variant="ghost"
                  onClick={() => void clearKey()}
                  data-testid="ai-key-clear"
                >
                  {t("ai.key.clear")}
                </Button>
              ) : null}
            </div>
            <p
              className="text-small text-saw-grey-700"
              data-testid="ai-key-status"
            >
              {settings.key_connected
                ? t("ai.key.connected")
                : t("ai.key.not_connected")}
            </p>
            {keySaved ? (
              <p
                role="status"
                className="rounded-card bg-saw-grey-100 px-3 py-2 text-small text-saw-grey-700"
              >
                {t("ai.key.connected")}
              </p>
            ) : null}
          </>
        ) : null}

        <hr className="border-saw-grey-100" />

        <div>
          <div className="font-medium text-saw-grey-900">
            {t("ai.context.title")}
          </div>
          <div className="text-small text-saw-grey-600 mt-1">
            {t("ai.context.subtitle")}
          </div>
        </div>

        <label className="flex flex-col gap-1 text-small text-saw-grey-700">
          <span>{t("ai.context.industry")}</span>
          <input
            type="text"
            value={context.industry}
            onChange={(e) =>
              setContext({ ...context, industry: e.target.value })
            }
            placeholder={t("ai.context.industry_placeholder")}
            className="rounded-card border border-saw-grey-200 bg-saw-white px-3 py-1.5 text-body text-saw-grey-900"
            data-testid="ai-ctx-industry"
          />
          {settings.flags.industry_identifying ? (
            <span
              className="text-xs text-saw-red"
              data-testid="ai-ctx-industry-warn"
            >
              {t("ai.context.industry_warn")}
            </span>
          ) : null}
        </label>

        <label className="flex flex-col gap-1 text-small text-saw-grey-700">
          <span>{t("ai.context.environment")}</span>
          <select
            value={context.environment_type}
            onChange={(e) =>
              setContext({
                ...context,
                environment_type: e.target.value as EnvironmentType,
              })
            }
            className="rounded-card border border-saw-grey-200 bg-saw-white px-3 py-1.5 text-body text-saw-grey-900"
            data-testid="ai-ctx-env"
          >
            {envOptions.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </select>
        </label>

        <label className="flex flex-col gap-1 text-small text-saw-grey-700">
          <span>{t("ai.context.compliance")}</span>
          <input
            type="text"
            value={complianceInput}
            onChange={(e) => setComplianceInput(e.target.value)}
            placeholder={t("ai.context.compliance_placeholder")}
            className="rounded-card border border-saw-grey-200 bg-saw-white px-3 py-1.5 text-body text-saw-grey-900 font-mono"
            data-testid="ai-ctx-compliance"
          />
          {settings.flags.compliance_identifying ? (
            <span
              className="text-xs text-saw-red"
              data-testid="ai-ctx-compliance-warn"
            >
              {t("ai.context.compliance_warn")}
            </span>
          ) : null}
        </label>

        <label className="flex flex-col gap-1 text-small text-saw-grey-700">
          <span>{t("ai.context.risk")}</span>
          <select
            value={context.risk_tolerance}
            onChange={(e) =>
              setContext({
                ...context,
                risk_tolerance: e.target.value as RiskTolerance,
              })
            }
            className="rounded-card border border-saw-grey-200 bg-saw-white px-3 py-1.5 text-body text-saw-grey-900"
            data-testid="ai-ctx-risk"
          >
            {riskOptions.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </select>
        </label>

        <label className="flex flex-col gap-1 text-small text-saw-grey-700">
          <span>{t("ai.context.team")}</span>
          <select
            value={context.team_size}
            onChange={(e) =>
              setContext({
                ...context,
                team_size: e.target.value as TeamSize,
              })
            }
            className="rounded-card border border-saw-grey-200 bg-saw-white px-3 py-1.5 text-body text-saw-grey-900"
            data-testid="ai-ctx-team"
          >
            {teamOptions.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </select>
        </label>

        <div>
          <Button
            variant="primary"
            onClick={() => void saveContext()}
            data-testid="ai-ctx-save"
          >
            {t("ai.context.save")}
          </Button>
        </div>
        {ctxSaved ? (
          <p
            role="status"
            className="rounded-card bg-saw-grey-100 px-3 py-2 text-small text-saw-grey-700"
            data-testid="ai-ctx-saved"
          >
            {t("ai.context.saved")}
          </p>
        ) : null}

        {err ? (
          <p role="alert" className="rounded-card bg-saw-grey-100 px-3 py-2 text-small text-saw-red">
            {err}
          </p>
        ) : null}
      </div>
    </section>
  );
}

// --- Report exporter section (Contract 15) -----------------------------

function ReportSection({
  onOpenCustomReport,
}: {
  onOpenCustomReport?: () => void;
}) {
  const t = useT();
  const formatError = useIpcError();
  const [settings, setSettings] = useState<ReportSettingsT | null>(null);
  const [busy, setBusy] = useState(false);
  const [pickerBusy, setPickerBusy] = useState(false);
  const [saved, setSaved] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      setSettings(await ipc.reportGetSettings());
    } catch (e) {
      setErr(formatError(e));
    }
  }, [formatError]);

  useEffect(() => {
    void reload();
  }, [reload]);

  if (!settings) return null;

  async function chooseFolder() {
    setPickerBusy(true);
    setErr(null);
    try {
      const picked = await openDialog({ directory: true, multiple: false });
      if (picked && typeof picked === "string") {
        setSettings({ ...settings!, auto_export_folder: picked });
      }
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setPickerBusy(false);
    }
  }

  async function persist() {
    setBusy(true);
    setSaved(false);
    setErr(null);
    try {
      await ipc.reportSetSettings(settings!);
      setSaved(true);
      window.setTimeout(() => setSaved(false), 2500);
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section
      className="mt-6 max-w-2xl rounded-card bg-saw-white border border-saw-grey-200 p-6"
      data-testid="settings-section-reports"
    >
      <h2 className="text-h3 font-semibold text-saw-grey-900">
        {t("report.settings.title")}
      </h2>
      <p className="mt-1 text-small text-saw-grey-600">
        {t("report.settings.subtitle")}
      </p>

      <div className="mt-4 flex flex-col gap-4">
        {onOpenCustomReport ? (
          <div>
            <Button
              variant="secondary"
              onClick={onOpenCustomReport}
              data-testid="settings-open-custom-report"
            >
              {t("report.custom.cta")}
            </Button>
          </div>
        ) : null}

        <label className="flex items-start gap-2 text-small text-saw-grey-700">
          <input
            type="checkbox"
            checked={settings.auto_export_enabled}
            onChange={(e) =>
              setSettings({ ...settings, auto_export_enabled: e.target.checked })
            }
            className="mt-1"
            data-testid="settings-reports-enable"
          />
          <span>{t("report.settings.enable")}</span>
        </label>

        <label className="flex flex-col gap-1 text-small text-saw-grey-700">
          <span>{t("report.settings.folder_label")}</span>
          <input
            type="text"
            readOnly
            value={settings.auto_export_folder ?? ""}
            placeholder={t("report.settings.folder_placeholder")}
            className="rounded-card border border-saw-grey-200 bg-saw-grey-50 px-3 py-1.5 text-body text-saw-grey-900 font-mono"
            data-testid="settings-reports-folder"
          />
        </label>
        <div>
          <Button
            variant="secondary"
            onClick={() => void chooseFolder()}
            disabled={pickerBusy}
            data-testid="settings-reports-choose-folder"
          >
            {t("report.settings.choose_folder")}
          </Button>
        </div>

        <label className="flex items-start gap-2 text-small text-saw-grey-700">
          <input
            type="checkbox"
            checked={settings.mask_account_ids_default}
            onChange={(e) =>
              setSettings({ ...settings, mask_account_ids_default: e.target.checked })
            }
            className="mt-1"
            data-testid="settings-reports-mask-default"
          />
          <span>{t("report.settings.mask_default")}</span>
        </label>

        <div>
          <Button
            variant="primary"
            onClick={() => void persist()}
            disabled={busy}
            data-testid="settings-reports-save"
          >
            {t("report.settings.save")}
          </Button>
        </div>
        {saved ? (
          <p
            role="status"
            className="rounded-card bg-saw-grey-100 px-3 py-2 text-small text-saw-grey-700"
            data-testid="settings-reports-saved"
          >
            {t("report.settings.saved")}
          </p>
        ) : null}
        {err ? (
          <p role="alert" className="rounded-card bg-saw-grey-100 px-3 py-2 text-small text-saw-red">
            {err}
          </p>
        ) : null}
      </div>
    </section>
  );
}
