import { useEffect, useState } from "react";

import Badge from "@/components/Badge";
import Button from "@/components/Button";
import EmptyState from "@/components/EmptyState";
import { useT } from "@/hooks/useT";
import { useIpcError } from "@/hooks/useIpcError";
import { ipc } from "@/lib/ipc";
import { useLock } from "@/stores/lock";

type Props = {
  onOpenSettings: () => void;
  onOpenAccounts: () => void;
};

export default function Home({ onOpenSettings, onOpenAccounts }: Props) {
  const t = useT();
  const formatError = useIpcError();
  const { refresh } = useLock();
  const [version, setVersion] = useState<string | null>(null);
  const [versionError, setVersionError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    ipc
      .appVersion()
      .then((v) => {
        if (!cancelled) setVersion(v);
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        const msg =
          typeof err === "object" && err !== null && "message" in err
            ? String((err as { message: unknown }).message)
            : t("common.error_generic");
        setVersionError(msg);
      });
    return () => {
      cancelled = true;
    };
  }, [t]);

  async function onLockNow() {
    try {
      await ipc.applockLock();
      await refresh();
    } catch (err) {
      // No dedicated error surface on Home; swallow to a console diagnostic
      // string. The next state read by LockProvider will resync.
      console.error(formatError(err));
    }
  }

  return (
    <main className="min-h-full bg-saw-grey-50 text-saw-grey-900">
      <header className="border-b border-saw-grey-200 bg-saw-white px-8 py-5">
        <div className="flex items-center gap-3">
          <div
            className="h-7 w-7 rounded-card bg-saw-red"
            aria-hidden="true"
          />
          <div className="flex flex-col">
            <h1 className="text-h2 font-semibold tracking-tight">
              {t("app.name")}
            </h1>
            <p className="text-small text-saw-grey-500">{t("app.tagline")}</p>
          </div>
          <div className="ml-auto flex items-center gap-2 text-small text-saw-grey-500">
            <Button
              variant="ghost"
              size="sm"
              onClick={onOpenAccounts}
              data-testid="header-accounts"
            >
              {t("nav.accounts")}
            </Button>
            <Button
              variant="ghost"
              size="sm"
              onClick={onLockNow}
              data-testid="header-lock-now"
            >
              {t("applock.settings.lock_now")}
            </Button>
            <Button
              variant="ghost"
              size="sm"
              onClick={onOpenSettings}
              data-testid="header-settings"
            >
              {t("nav.settings")}
            </Button>
            <span>{t("app.version_label")}</span>
            {version ? (
              <Badge tone="neutral" data-testid="app-version">
                {version}
              </Badge>
            ) : versionError ? (
              <Badge tone="danger" data-testid="app-version-error">
                {versionError}
              </Badge>
            ) : (
              <Badge tone="neutral">{t("common.loading")}</Badge>
            )}
          </div>
        </div>
      </header>

      <section className="mx-auto max-w-4xl px-8 py-10">
        <h2 className="text-display font-semibold tracking-tight">
          {t("home.heading")}
        </h2>
        <p className="mt-3 max-w-2xl text-body text-saw-grey-600">
          {t("home.body")}
        </p>

        <div className="mt-8">
          <EmptyState
            title={t("empty.no_scans.title")}
            body={t("empty.no_scans.body")}
            action={
              <Button
                variant="primary"
                type="button"
                onClick={onOpenAccounts}
                data-testid="empty-open-accounts"
              >
                {t("nav.accounts")}
              </Button>
            }
          />
        </div>
      </section>
    </main>
  );
}
