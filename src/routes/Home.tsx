// Dashboard / Welcome page — PR #50.
//
// Repurposed from the original Home.tsx (nav-buttons-only landing).
// The persistent TopNav "Dashboard" button routes here. Surfaces:
//
//   1. Scan Now CTA (top, prominent) — opens the global ScanModal
//      (PR #39's <ScanModalProvider>) without leaving this page.
//   2. Recent activity card — last 5 scans across all configured
//      accounts, newest first.
//   3. Top findings card — top 5 highest-severity findings from
//      the most recent terminal scan across all accounts.
//
// Empty states cover both first-run ("no accounts configured —
// go to Settings → Accounts") and post-onboarding-but-pre-scan
// ("no scans yet — click Scan Now to get started").
//
// All data flows through existing IPC (no new commands):
//   - `ipc.accountsList()` — once on mount
//   - `ipc.scannerListRecent(awsAccountId, 5)` — per account, parallel
//   - `ipc.findingsList(scanId)` — once for the latest terminal scan
//
// Subscribes to `SCAN_FINISHED_EVENT` from the ScanModalProvider so
// the data refreshes immediately after a scan completes (without
// requiring the user to leave and return).

import { useCallback, useEffect, useState } from "react";

import {
  Badge,
  Button,
  EmptyState,
  Logo,
  SeverityBadge,
} from "@/components";
import { useT } from "@/hooks/useT";
import { useIpcError } from "@/hooks/useIpcError";
import {
  ipc,
  maskAccountId,
  SEVERITY_ORDER,
  type Account,
  type Finding,
  type ScanRecord,
  type Severity,
} from "@/lib/ipc";
import {
  SCAN_FINISHED_EVENT,
  useScanModal,
} from "@/contexts/ScanModalContext";

type Props = {
  /** Navigate to Settings (used by empty-state CTAs when the user
   *  needs to configure an account). The persistent TopNav already
   *  has a Settings button — these CTAs are convenience shortcuts
   *  for the in-content empty states. */
  onOpenSettings: () => void;
};

const RECENT_LIMIT = 5;
const TOP_FINDINGS_LIMIT = 5;

export default function Home({ onOpenSettings }: Props) {
  const t = useT();
  const formatError = useIpcError();
  const { open: openScanModal } = useScanModal();

  const [accounts, setAccounts] = useState<Account[] | null>(null);
  const [recentScans, setRecentScans] = useState<ScanRecord[] | null>(null);
  const [topFindings, setTopFindings] = useState<Finding[] | null>(null);
  const [latestScan, setLatestScan] = useState<ScanRecord | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    setLoadError(null);
    try {
      // 1. Accounts. If none, both downstream queries become empty.
      const acctList = await ipc.accountsList();
      setAccounts(acctList);

      if (acctList.length === 0) {
        setRecentScans([]);
        setTopFindings([]);
        setLatestScan(null);
        return;
      }

      // 2. Recent scans across all accounts — parallel fetch then
      //    merge + sort by started_at (newest first), take top N.
      const perAccount = await Promise.all(
        acctList.map((a) =>
          ipc.scannerListRecent(a.aws_account_id, RECENT_LIMIT),
        ),
      );
      const merged = perAccount.flat().sort((a, b) => {
        return (
          new Date(b.started_at).getTime() - new Date(a.started_at).getTime()
        );
      });
      setRecentScans(merged.slice(0, RECENT_LIMIT));

      // 3. Top findings — pick the most recent scan that reached a
      //    terminal state with output (complete / complete_with_warnings)
      //    and fetch its findings.
      const terminal = merged.find(
        (s) =>
          (s.status === "complete" ||
            s.status === "complete_with_warnings") &&
          s.raw_output_path !== null,
      );
      setLatestScan(terminal ?? null);
      if (terminal) {
        const findings = await ipc.findingsList(terminal.scan_id);
        // Sort by severity (critical first). SEVERITY_ORDER is the
        // canonical worst→best ranking.
        const ranked = [...findings].sort((a, b) => {
          return (
            SEVERITY_ORDER.indexOf(a.severity) -
            SEVERITY_ORDER.indexOf(b.severity)
          );
        });
        setTopFindings(ranked.slice(0, TOP_FINDINGS_LIMIT));
      } else {
        setTopFindings([]);
      }
    } catch (err) {
      setLoadError(formatError(err));
      // Don't clobber whatever loaded successfully so partial data
      // stays visible; just surface the error banner.
    }
  }, [formatError]);

  useEffect(() => {
    void reload();
  }, [reload]);

  // Refresh when any scan (from anywhere in the app) reaches a
  // terminal state via the global ScanModalProvider.
  useEffect(() => {
    const handler = () => void reload();
    document.addEventListener(SCAN_FINISHED_EVENT, handler);
    return () => document.removeEventListener(SCAN_FINISHED_EVENT, handler);
  }, [reload]);

  const hasAccounts = accounts !== null && accounts.length > 0;
  const isLoading = accounts === null;

  return (
    <main className="min-h-full bg-saw-grey-50 dark:bg-saw-black text-saw-grey-900 dark:text-saw-beige">
      <header className="border-b border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-8 py-5">
        <div className="flex items-center gap-3">
          <Logo size="sm" />
          <div className="flex flex-col">
            <h1 className="text-h2 font-semibold tracking-tight">
              {t("dashboard.welcome.title")}
            </h1>
            <p className="text-small text-saw-grey-500 dark:text-saw-grey-400">{t("app.tagline")}</p>
          </div>
        </div>
      </header>

      <section className="mx-auto max-w-7xl px-8 py-10">
        {/* Scan Now CTA — the primary action on the page. Opens
            the global ScanModal which contains its own account
            picker (PR #39) so the user can scan from here without
            navigating to Accounts/Settings first. */}
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div>
            <h2 className="text-h2 font-semibold tracking-tight">
              {t("dashboard.welcome.scan_heading")}
            </h2>
            <p className="mt-1 text-small text-saw-grey-600 dark:text-saw-grey-400">
              {t("dashboard.welcome.scan_subtitle")}
            </p>
          </div>
          <Button
            variant="primary"
            size="lg"
            onClick={() => openScanModal()}
            disabled={!hasAccounts}
            data-testid="dashboard-scan-now"
          >
            {t("dashboard.welcome.scan_now")}
          </Button>
        </div>

        {loadError ? (
          <p
            role="alert"
            className="mt-6 rounded-card border border-saw-red/40 bg-saw-red/5 px-4 py-3 text-body text-saw-grey-900 dark:text-saw-beige"
            data-testid="dashboard-error"
          >
            {loadError}
          </p>
        ) : null}

        {/* First-run empty state: no accounts configured at all. */}
        {!isLoading && !hasAccounts ? (
          <div className="mt-8">
            <EmptyState
              title={t("dashboard.welcome.no_accounts.title")}
              body={t("dashboard.welcome.no_accounts.body")}
              action={
                <Button
                  variant="primary"
                  onClick={onOpenSettings}
                  data-testid="dashboard-open-settings"
                >
                  {t("dashboard.welcome.no_accounts.cta")}
                </Button>
              }
            />
          </div>
        ) : null}

        {hasAccounts ? (
          // PR #71: Top findings now renders FIRST. Recent activity
          // is secondary context; the dominant card on the dashboard
          // should be "what's broken right now," not "what did I do
          // recently."
          <div className="mt-8 grid gap-6 lg:grid-cols-2">
            <TopFindingsCard
              findings={topFindings}
              latestScan={latestScan}
            />
            <RecentActivityCard
              scans={recentScans}
              accounts={accounts ?? []}
            />
          </div>
        ) : null}
      </section>
    </main>
  );
}

function RecentActivityCard({
  scans,
  accounts,
}: {
  scans: ScanRecord[] | null;
  accounts: Account[];
}) {
  const t = useT();

  return (
    <div
      className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark p-6"
      data-testid="dashboard-recent-activity"
    >
      <h3 className="text-h3 font-semibold text-saw-grey-900 dark:text-saw-beige">
        {t("dashboard.recent_activity.title")}
      </h3>

      {scans === null ? (
        <p className="mt-3 text-small text-saw-grey-600 dark:text-saw-grey-400">
          {t("common.loading")}
        </p>
      ) : scans.length === 0 ? (
        <p
          className="mt-3 text-small text-saw-grey-600"
          data-testid="dashboard-recent-activity-empty"
        >
          {t("dashboard.recent_activity.empty")}
        </p>
      ) : (
        <ul className="mt-3 flex flex-col divide-y divide-saw-grey-100 dark:divide-saw-grey-800">
          {scans.map((s) => {
            const acct = accounts.find(
              (a) => a.aws_account_id === s.aws_account_id,
            );
            const label = acct?.label ?? maskAccountId(s.aws_account_id);
            return (
              <li
                key={s.scan_id}
                className="flex items-center justify-between gap-3 py-3"
                data-testid="dashboard-recent-activity-row"
              >
                <div className="flex flex-col">
                  <span className="text-small font-medium text-saw-grey-900 dark:text-saw-beige">
                    {label}
                  </span>
                  <span className="text-xs text-saw-grey-500 dark:text-saw-grey-400">
                    {formatTimestamp(s.started_at)}
                  </span>
                </div>
                <ScanStatusBadge status={s.status} />
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}

function TopFindingsCard({
  findings,
  latestScan,
}: {
  findings: Finding[] | null;
  latestScan: ScanRecord | null;
}) {
  const t = useT();

  return (
    <div
      className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark p-6"
      data-testid="dashboard-top-findings"
    >
      <h3 className="text-h3 font-semibold text-saw-grey-900 dark:text-saw-beige">
        {t("dashboard.top_findings.title")}
      </h3>
      {latestScan ? (
        <p className="mt-1 text-xs text-saw-grey-500 dark:text-saw-grey-400">
          {t("dashboard.top_findings.from_scan").replace(
            "{timestamp}",
            formatTimestamp(latestScan.started_at),
          )}
        </p>
      ) : null}

      {findings === null ? (
        <p className="mt-3 text-small text-saw-grey-600 dark:text-saw-grey-400">
          {t("common.loading")}
        </p>
      ) : findings.length === 0 ? (
        <p
          className="mt-3 text-small text-saw-grey-600"
          data-testid="dashboard-top-findings-empty"
        >
          {t("dashboard.top_findings.empty")}
        </p>
      ) : (
        <ul className="mt-3 flex flex-col divide-y divide-saw-grey-100 dark:divide-saw-grey-800">
          {findings.map((f) => (
            <li
              key={f.finding_id}
              className="flex items-start gap-3 py-3"
              data-testid="dashboard-top-findings-row"
            >
              <SeverityBadge severity={f.severity} />
              <div className="flex min-w-0 flex-col">
                <span className="text-small font-medium text-saw-grey-900 dark:text-saw-beige truncate">
                  {f.dashboard_name ?? f.rule_key}
                </span>
                <span className="text-xs text-saw-grey-500 dark:text-saw-grey-400">
                  {f.service}
                </span>
              </div>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/** Compact status indicator for the recent-activity rows.
 *  Maps each ScanStatus to an existing Badge tone — no new
 *  semantic vocabulary. Could be promoted to a shared component
 *  later if other surfaces want the same mapping. */
function ScanStatusBadge({ status }: { status: ScanRecord["status"] }) {
  const t = useT();
  const tone: "success" | "info" | "danger" | "neutral" = (() => {
    switch (status) {
      case "complete":
        return "success";
      case "complete_with_warnings":
        return "info";
      case "failed":
        return "danger";
      case "canceled":
        return "neutral";
      default:
        return "info";
    }
  })();
  return <Badge tone={tone}>{t(`scanner.status.${status}`)}</Badge>;
}

function formatTimestamp(iso: string): string {
  try {
    return new Date(iso).toLocaleString();
  } catch {
    return iso;
  }
}

// `Severity` is imported only for its type — the SEVERITY_ORDER
// constant carries the value. Exported just to keep the type
// import line tidy; not used externally.
export type _SeverityForLint = Severity;
