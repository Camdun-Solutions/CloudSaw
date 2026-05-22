// Settings — lock period, biometric toggle, change-password.
//
// This is the only post-unlock screen Contract 02 owns. Later contracts will
// build out the full settings surface; for now Settings is a single panel
// dedicated to app-lock configuration, reachable from the main header.

import { useCallback, useEffect, useState } from "react";

import { Button, Modal, PasswordField, Select, Switch } from "@/components";
import { useT } from "@/hooks/useT";
import { useIpcError } from "@/hooks/useIpcError";
import {
  ipc,
  type GithubSettings,
  type LockPeriod,
  type LockSettings,
  type PanicWipeResult,
  type RetentionPeriod,
  type RetentionSettings,
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
};

export default function Settings({ onClose, onOpenSchedules, onOpenActivityLog }: Props) {
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
      <RetentionSection />
      <GithubSection />
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

// --- Contract 11 sections -----------------------------------------------

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
