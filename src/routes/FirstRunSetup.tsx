// First-run gate. The app cannot proceed until a master password is set
// (Contract 02 edge case: "Setup cannot be skipped"). The user picks a
// password, confirms it, and acknowledges the security disclosure.

import { useState, type FormEvent } from "react";

import { Button, Logo, PasswordField } from "@/components";
import { useT } from "@/hooks/useT";
import { useIpcError } from "@/hooks/useIpcError";
import { ipc } from "@/lib/ipc";
import { useLock } from "@/stores/lock";

const MIN_PASSWORD_LEN = 8;

export default function FirstRunSetup() {
  const t = useT();
  const formatError = useIpcError();
  const { refresh } = useLock();

  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const tooShort = password.length > 0 && password.length < MIN_PASSWORD_LEN;
  const mismatch =
    password.length > 0 && confirm.length > 0 && password !== confirm;
  const canSubmit =
    !submitting &&
    password.length >= MIN_PASSWORD_LEN &&
    password === confirm;

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    if (!canSubmit) return;
    setSubmitting(true);
    setError(null);
    try {
      await ipc.applockSetMasterPassword(password);
      // Wipe form state immediately — the password string lives only as long
      // as we need it. (The Rust side zeroizes its own copy.)
      setPassword("");
      setConfirm("");
      await refresh();
    } catch (err) {
      setError(formatError(err));
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <main className="min-h-full bg-saw-grey-50 dark:bg-saw-black flex items-center justify-center px-6 py-12">
      <div className="w-full max-w-md rounded-card bg-saw-white dark:bg-saw-grey-dark p-8 shadow-sm border border-saw-grey-200 dark:border-saw-grey-700">
        <div className="flex items-center gap-3">
          <Logo size="sm" />
          <h1 className="text-h2 font-semibold text-saw-grey-900 dark:text-saw-beige">
            {t("applock.setup.title")}
          </h1>
        </div>
        <p className="mt-4 text-body text-saw-grey-700 dark:text-saw-grey-300">
          {t("applock.setup.subtitle")}
        </p>
        <p className="mt-3 rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-grey-700 dark:text-saw-grey-300">
          {t("applock.disclosure")}
        </p>

        <form className="mt-6 flex flex-col gap-4" onSubmit={onSubmit}>
          <PasswordField
            label={t("applock.field.new_password")}
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            autoComplete="new-password"
            hint={t("applock.setup.password_hint").replace(
              "{min}",
              String(MIN_PASSWORD_LEN),
            )}
            error={tooShort ? t("applock.error.too_short") : null}
            showLabel={t("applock.field.show")}
            hideLabel={t("applock.field.hide")}
            data-testid="setup-password"
          />
          <PasswordField
            label={t("applock.field.confirm_password")}
            value={confirm}
            onChange={(e) => setConfirm(e.target.value)}
            autoComplete="new-password"
            error={mismatch ? t("applock.error.mismatch") : null}
            showLabel={t("applock.field.show")}
            hideLabel={t("applock.field.hide")}
            data-testid="setup-confirm"
          />

          {error ? (
            <p
              role="alert"
              className="rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-red"
            >
              {error}
            </p>
          ) : null}

          <Button
            type="submit"
            variant="primary"
            disabled={!canSubmit}
            data-testid="setup-submit"
          >
            {submitting
              ? t("applock.setup.submitting")
              : t("applock.setup.submit")}
          </Button>
        </form>
      </div>
    </main>
  );
}
