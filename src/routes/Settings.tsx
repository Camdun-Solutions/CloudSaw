// Settings — lock period, biometric toggle, change-password.
//
// This is the only post-unlock screen Contract 02 owns. Later contracts will
// build out the full settings surface; for now Settings is a single panel
// dedicated to app-lock configuration, reachable from the main header.

import { useEffect, useState } from "react";

import { Button, Modal, PasswordField, Select, Switch } from "@/components";
import { useT } from "@/hooks/useT";
import { useIpcError } from "@/hooks/useIpcError";
import { ipc, type LockPeriod, type LockSettings } from "@/lib/ipc";
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

type Props = { onClose: () => void; onOpenSchedules: () => void };

export default function Settings({ onClose, onOpenSchedules }: Props) {
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
