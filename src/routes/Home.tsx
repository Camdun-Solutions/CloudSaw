import Button from "@/components/Button";
import EmptyState from "@/components/EmptyState";
import Logo from "@/components/Logo";
import { useT } from "@/hooks/useT";

type Props = {
  onOpenSettings: () => void;
  onOpenAccounts: () => void;
  onOpenDashboard: () => void;
};

export default function Home({
  onOpenSettings,
  onOpenAccounts,
  onOpenDashboard,
}: Props) {
  const t = useT();


  return (
    <main className="min-h-full bg-saw-grey-50 text-saw-grey-900">
      <header className="border-b border-saw-grey-200 bg-saw-white px-8 py-5">
        <div className="flex items-center gap-3">
          <Logo size="sm" />
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
              onClick={onOpenDashboard}
              data-testid="header-dashboard"
            >
              {t("nav.dashboard")}
            </Button>
            <Button
              variant="ghost"
              size="sm"
              onClick={onOpenAccounts}
              data-testid="header-accounts"
            >
              {t("nav.accounts")}
            </Button>
            {/* "Lock now" used to live here as a text button — PR
                #42 moved it to a lock icon in the persistent TopNav
                (top-right corner, always visible). The Settings
                button below is one of the per-route duplicates
                slated for removal in PR #44 once the TopNav is
                fully verified. */}
            <Button
              variant="ghost"
              size="sm"
              onClick={onOpenSettings}
              data-testid="header-settings"
            >
              {t("nav.settings")}
            </Button>
            {/* Version badge previously lived here. Moved to the
                global bottom-left <VersionFooter /> in PR #43 so
                the version stays visible across every route, not
                just Home. */}
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
