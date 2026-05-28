// The unlock gate. Shown when the app is locked but a password has been set.
//
// Three paths:
//   * Password input — always available.
//   * Biometric button — visible only when biometrics is enabled AND the
//     platform reports availability.
//   * "Forgot password?" — opens the recovery dialog, which triggers the OS
//     identity prompt (Windows Hello / device password / passkey) and, on
//     success, lets the user set a new password without revealing the old one.

import { useEffect, useState, type FormEvent } from "react";

import { Button, Logo, Modal, PasswordField } from "@/components";
import { useT } from "@/hooks/useT";
import { useIpcError } from "@/hooks/useIpcError";
import { ipc } from "@/lib/ipc";
import { useLock } from "@/stores/lock";

const MIN_PASSWORD_LEN = 8;

export default function UnlockScreen() {
  const t = useT();
  const formatError = useIpcError();
  const { state, refresh } = useLock();

  const [password, setPassword] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [biometricBusy, setBiometricBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [recoveryOpen, setRecoveryOpen] = useState(false);

  const biometricVisible =
    !!state?.settings.biometric_enabled &&
    state?.biometric_availability === "Available";

  const recoveryAvailable = !!state?.recovery_available;

  // Auto-trigger biometric on mount when it's the user's primary path. We
  // gate behind a one-shot flag so a denied prompt doesn't re-trigger in a
  // loop.
  const [autoBioTried, setAutoBioTried] = useState(false);
  useEffect(() => {
    if (!biometricVisible || autoBioTried) return;
    setAutoBioTried(true);
    void onBiometric();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [biometricVisible, autoBioTried]);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    if (submitting || password.length === 0) return;
    setSubmitting(true);
    setError(null);
    try {
      await ipc.applockUnlock(password);
      setPassword("");
      await refresh();
    } catch (err) {
      setError(formatError(err));
    } finally {
      setSubmitting(false);
    }
  }

  async function onBiometric() {
    if (biometricBusy) return;
    setBiometricBusy(true);
    setError(null);
    try {
      await ipc.applockUnlockWithBiometric(t("applock.biometric.reason"));
      await refresh();
    } catch (err) {
      setError(formatError(err));
    } finally {
      setBiometricBusy(false);
    }
  }

  return (
    <main className="min-h-full bg-saw-grey-50 dark:bg-saw-black flex items-center justify-center px-6 py-12">
      <div className="w-full max-w-md rounded-card bg-saw-white dark:bg-saw-grey-dark p-8 shadow-sm border border-saw-grey-200 dark:border-saw-grey-700">
        <div className="flex items-center gap-3">
          <Logo size="sm" />
          <h1 className="text-h2 font-semibold text-saw-grey-900 dark:text-saw-beige">
            {t("applock.unlock.title")}
          </h1>
        </div>
        <p className="mt-4 text-body text-saw-grey-700 dark:text-saw-grey-300">
          {t("applock.unlock.subtitle")}
        </p>

        <form className="mt-6 flex flex-col gap-4" onSubmit={onSubmit}>
          <PasswordField
            label={t("applock.field.password")}
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            autoComplete="current-password"
            autoFocus
            showLabel={t("applock.field.show")}
            hideLabel={t("applock.field.hide")}
            data-testid="unlock-password"
          />
          {error ? (
            <p
              role="alert"
              className="rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-red"
              data-testid="unlock-error"
            >
              {error}
            </p>
          ) : null}
          <Button
            type="submit"
            variant="primary"
            disabled={submitting || password.length === 0}
            data-testid="unlock-submit"
          >
            {submitting
              ? t("applock.unlock.submitting")
              : t("applock.unlock.submit")}
          </Button>

          {biometricVisible ? (
            <Button
              type="button"
              variant="secondary"
              disabled={biometricBusy}
              onClick={onBiometric}
              data-testid="unlock-biometric"
            >
              {biometricBusy
                ? t("applock.biometric.busy")
                : t("applock.biometric.use")}
            </Button>
          ) : null}
        </form>

        <div className="mt-6 flex items-center justify-between text-small">
          <button
            type="button"
            className="text-saw-grey-600 dark:text-saw-grey-400 hover:text-saw-grey-900 dark:hover:text-saw-beige underline decoration-saw-grey-400 underline-offset-2 disabled:no-underline disabled:text-saw-grey-400 disabled:cursor-not-allowed"
            onClick={() => setRecoveryOpen(true)}
            disabled={!recoveryAvailable}
            title={
              recoveryAvailable
                ? undefined
                : t("applock.recovery.unavailable_hint")
            }
            data-testid="unlock-forgot"
          >
            {t("applock.unlock.forgot")}
          </button>
        </div>
      </div>

      <RecoveryDialog
        open={recoveryOpen}
        onClose={() => setRecoveryOpen(false)}
        onUnlocked={async () => {
          setRecoveryOpen(false);
          await refresh();
        }}
      />
    </main>
  );
}

function RecoveryDialog({
  open,
  onClose,
  onUnlocked,
}: {
  open: boolean;
  onClose: () => void;
  onUnlocked: () => Promise<void>;
}) {
  const t = useT();
  const formatError = useIpcError();
  const [newPassword, setNewPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) {
      setNewPassword("");
      setConfirm("");
      setError(null);
      setBusy(false);
    }
  }, [open]);

  const tooShort =
    newPassword.length > 0 && newPassword.length < MIN_PASSWORD_LEN;
  const mismatch =
    newPassword.length > 0 && confirm.length > 0 && newPassword !== confirm;
  const canSubmit =
    !busy && newPassword.length >= MIN_PASSWORD_LEN && newPassword === confirm;

  async function onSubmit() {
    if (!canSubmit) return;
    setBusy(true);
    setError(null);
    try {
      await ipc.applockRecoveryUnlock(newPassword, t("applock.recovery.reason"));
      setNewPassword("");
      setConfirm("");
      await onUnlocked();
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
      title={t("applock.recovery.title")}
      footer={
        <>
          <Button variant="ghost" onClick={onClose} disabled={busy}>
            {t("common.cancel")}
          </Button>
          <Button
            variant="primary"
            onClick={onSubmit}
            disabled={!canSubmit}
            data-testid="recovery-submit"
          >
            {busy ? t("applock.recovery.busy") : t("applock.recovery.submit")}
          </Button>
        </>
      }
    >
      <p>{t("applock.recovery.explainer")}</p>
      <div className="mt-4 flex flex-col gap-4">
        <PasswordField
          label={t("applock.field.new_password")}
          value={newPassword}
          onChange={(e) => setNewPassword(e.target.value)}
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
