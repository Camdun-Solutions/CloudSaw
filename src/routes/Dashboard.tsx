// Dashboard — scan history, findings, drift, and trends (Contract 09).
//
// This single route hosts four sub-views that share the same active-account
// context and account-scoped data fetches:
//
//   - scans     — `/scans` equivalent: scan history for the active account.
//   - findings  — `/scans/:scanId` equivalent: split list+detail view.
//   - drift     — cross-scan diff between two selected scans.
//   - trends    — severity counts over time + MTTR + per-finding timelines.
//
// All backend access goes through `ipc` (no direct `invoke()` in this tree,
// per Contract 09 §Constraints + CLAUDE.md §4.1).

import { useCallback, useEffect, useMemo, useState } from "react";

import {
  Badge,
  Button,
  EmptyState,
  LineChart,
  Modal,
  SeverityBadge,
  Switch,
} from "@/components";
import FindingsView from "@/routes/dashboard/FindingsView";
import DriftView from "@/routes/dashboard/DriftView";
import TrendsView from "@/routes/dashboard/TrendsView";
import { useT } from "@/hooks/useT";
import { useIpcError } from "@/hooks/useIpcError";
import {
  ipc,
  isTerminalScanStatus,
  maskAccountId,
  SEVERITY_ORDER,
  type Account,
  type AccountsDisplaySettings,
  type Finding,
  type HardDeleteSummary,
  type ScanRecord,
  type ScanStatus,
  type Severity,
} from "@/lib/ipc";

type Props = {
  onClose: () => void;
  onOpenAccounts: () => void;
  /** Open the global error-report dialog with the supplied notes
   * pre-filled. Optional; the dashboard's load-error UI uses it. */
  onOpenReport?: (notes?: string) => void;
};

type Tab = "scans" | "findings" | "drift" | "trends";

export default function Dashboard({ onClose, onOpenAccounts, onOpenReport }: Props) {
  const t = useT();
  const formatError = useIpcError();

  const [tab, setTab] = useState<Tab>("scans");
  const [account, setAccount] = useState<Account | null>(null);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [display, setDisplay] = useState<AccountsDisplaySettings>({
    reveal_full_ids: false,
  });
  const [loadError, setLoadError] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);

  const [scans, setScans] = useState<ScanRecord[] | null>(null);
  const [scanCounts, setScanCounts] = useState<
    Record<string, Record<Severity, number>>
  >({});
  const [selectedScanId, setSelectedScanId] = useState<string | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<ScanRecord | null>(null);
  const [deleteToast, setDeleteToast] = useState<string | null>(null);

  const loadAccount = useCallback(async () => {
    setLoadError(null);
    try {
      const [active, settings] = await Promise.all([
        ipc.accountsGetActive(),
        ipc.accountsGetDisplaySettings(),
      ]);
      setActiveId(active);
      setDisplay(settings);
      if (active) {
        const acc = await ipc.accountsGet(active);
        setAccount(acc);
      } else {
        setAccount(null);
      }
    } catch (err) {
      setLoadError(formatError(err));
    } finally {
      setLoaded(true);
    }
  }, [formatError]);

  useEffect(() => {
    void loadAccount();
  }, [loadAccount]);

  // Pull scans whenever the active account changes.
  useEffect(() => {
    if (!activeId) {
      setScans(null);
      setScanCounts({});
      return;
    }
    let cancelled = false;
    setScans(null);
    ipc
      .findingsListScans(activeId)
      .then((list) => {
        if (cancelled) return;
        setScans(list);
        // For each terminal scan, fire-and-forget a severity-count summary.
        list
          .filter((s) => isTerminalScanStatus(s.status))
          .forEach((s) => {
            ipc
              .findingsList(s.scan_id)
              .then((findings) => {
                if (cancelled) return;
                setScanCounts((prev) => ({
                  ...prev,
                  [s.scan_id]: countBySeverity(findings),
                }));
              })
              .catch(() => {
                /* Per-scan summary is best-effort; the row falls back to
                 * "loading severity counts" until/unless this succeeds. */
              });
          });
      })
      .catch((err) => {
        if (cancelled) return;
        setLoadError(formatError(err));
        setScans([]);
      });
    return () => {
      cancelled = true;
    };
  }, [activeId, formatError]);

  const showId = (id: string) => (display.reveal_full_ids ? id : maskAccountId(id));

  if (!loaded) {
    return (
      <main className="min-h-full bg-saw-grey-50 flex items-center justify-center">
        <p className="text-body text-saw-grey-600">{t("common.loading")}</p>
      </main>
    );
  }

  async function onConfirmDelete(
    target: ScanRecord,
    confirmation: string,
    secureOverwrite: boolean,
  ): Promise<HardDeleteSummary> {
    const summary = await ipc.deletionHardDeleteScan(target.scan_id, confirmation, {
      secure_overwrite: secureOverwrite,
    });
    setDeleteTarget(null);
    setDeleteToast(
      t("delete.scan.success").replace("{findings}", String(summary.findings_removed)),
    );
    if (activeId) {
      ipc.findingsListScans(activeId).then(setScans).catch(() => undefined);
    }
    window.setTimeout(() => setDeleteToast(null), 4000);
    return summary;
  }

  return (
    <main className="min-h-full bg-saw-grey-50 text-saw-grey-900">
      <HardDeleteDialog
        target={deleteTarget}
        account={account}
        onCancel={() => setDeleteTarget(null)}
        onConfirm={onConfirmDelete}
        showId={showId}
      />
      <header className="border-b border-saw-grey-200 bg-saw-white px-8 py-5">
        <div className="flex items-center gap-3">
          <div
            className="h-7 w-7 rounded-card bg-saw-red"
            aria-hidden="true"
          />
          <div className="flex flex-col">
            <h1 className="text-h2 font-semibold tracking-tight">
              {t("dashboard.title")}
            </h1>
            <p className="text-small text-saw-grey-500">
              {t("dashboard.subtitle")}
            </p>
          </div>
          <div className="ml-auto flex items-center gap-2">
            {account ? (
              <Badge tone="info" data-testid="dashboard-active-account">
                {account.label} · {showId(account.aws_account_id)}
              </Badge>
            ) : null}
            <Button
              variant="ghost"
              size="sm"
              onClick={onClose}
              data-testid="dashboard-close"
            >
              {t("common.close")}
            </Button>
          </div>
        </div>

        <nav
          className="mt-4 flex gap-1"
          role="tablist"
          aria-label={t("dashboard.title")}
        >
          {(["scans", "findings", "drift", "trends"] as Tab[]).map((id) => {
            const selected = tab === id;
            return (
              <button
                key={id}
                type="button"
                role="tab"
                aria-selected={selected}
                onClick={() => {
                  if (id === "findings" && !selectedScanId) {
                    setTab("scans");
                  } else {
                    setTab(id);
                  }
                }}
                data-testid={`dashboard-tab-${id}`}
                className={[
                  "rounded-card px-3 py-1.5 text-small font-medium transition-colors",
                  selected
                    ? "bg-saw-grey-900 text-saw-white"
                    : "bg-transparent text-saw-grey-700 hover:bg-saw-grey-100",
                ].join(" ")}
              >
                {t(`dashboard.tab.${id}`)}
              </button>
            );
          })}
        </nav>
      </header>

      <section className="mx-auto max-w-6xl px-8 py-8">
        {loadError ? (
          <div
            role="alert"
            className="mb-4 rounded-card border border-saw-red/40 bg-saw-red/5 px-4 py-3 text-body text-saw-grey-900"
            data-testid="dashboard-error"
          >
            {loadError}
            <CopyDiagnostic info={loadError} />
            {onOpenReport ? (
              <button
                type="button"
                onClick={() => onOpenReport(loadError)}
                className="ml-3 text-small text-saw-grey-700 underline underline-offset-2"
                data-testid="dashboard-report-error"
              >
                {t("errordialog.file_bug")}
              </button>
            ) : null}
          </div>
        ) : null}

        {!account ? (
          <EmptyState
            title={t("dashboard.no_active_account.title")}
            body={t("dashboard.no_active_account.body")}
            action={
              <Button
                variant="primary"
                onClick={onOpenAccounts}
                data-testid="dashboard-open-accounts"
              >
                {t("dashboard.no_active_account.cta")}
              </Button>
            }
          />
        ) : tab === "scans" ? (
          <ScansView
            account={account}
            scans={scans}
            scanCounts={scanCounts}
            onOpenScan={(id) => {
              setSelectedScanId(id);
              setTab("findings");
            }}
            onOpenAccounts={onOpenAccounts}
            onDeleteScan={(s) => setDeleteTarget(s)}
            showId={showId}
            deleteToast={deleteToast}
            onDismissToast={() => setDeleteToast(null)}
          />
        ) : tab === "findings" ? (
          <FindingsView
            scanId={selectedScanId}
            onBack={() => setTab("scans")}
          />
        ) : tab === "drift" ? (
          <DriftView account={account} scans={scans ?? []} />
        ) : (
          <TrendsView account={account} scans={scans ?? []} />
        )}
      </section>
    </main>
  );
}

// ----- Scans tab ----------------------------------------------------------

type ScansViewProps = {
  account: Account;
  scans: ScanRecord[] | null;
  scanCounts: Record<string, Record<Severity, number>>;
  onOpenScan: (scanId: string) => void;
  onOpenAccounts: () => void;
  onDeleteScan: (scan: ScanRecord) => void;
  showId: (id: string) => string;
  deleteToast: string | null;
  onDismissToast: () => void;
};

function ScansView({
  account,
  scans,
  scanCounts,
  onOpenScan,
  onOpenAccounts,
  onDeleteScan,
  showId,
  deleteToast,
  onDismissToast,
}: ScansViewProps) {
  const t = useT();
  // Hook must run unconditionally — compute the chart series first, then
  // branch on the load state.
  const chartSeries = useChartSeriesFromScans(scans ?? [], scanCounts);

  if (scans === null) {
    return <p className="text-body text-saw-grey-600">{t("common.loading")}</p>;
  }

  if (scans.length === 0) {
    return (
      <EmptyState
        title={t("dashboard.scans.empty.title")}
        body={t("dashboard.scans.empty.body")}
        action={
          <Button
            variant="primary"
            onClick={onOpenAccounts}
            data-testid="dashboard-scans-open-accounts"
          >
            {t("dashboard.scans.empty.cta")}
          </Button>
        }
      />
    );
  }

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h2 className="text-h2 font-semibold">
          {t("dashboard.scans.title")}
        </h2>
        <Button
          variant="secondary"
          onClick={onOpenAccounts}
          data-testid="dashboard-scans-new"
        >
          {t("dashboard.scans.new_scan_cta")}
        </Button>
      </div>
      <p className="text-small text-saw-grey-600">
        {t("dashboard.scans.subtitle").replace("{account}", account.label)}
      </p>

      {deleteToast ? (
        <p
          role="status"
          className="rounded-card bg-saw-grey-100 px-3 py-2 text-small text-saw-grey-700"
          data-testid="dashboard-delete-toast"
          onClick={onDismissToast}
        >
          {deleteToast}
        </p>
      ) : null}

      <LineChart
        ariaTitle={t("dashboard.drift.chart.title")}
        series={chartSeries}
      />

      <div
        role="table"
        aria-label={t("dashboard.scans.title")}
        className="rounded-card border border-saw-grey-200 bg-saw-white overflow-hidden"
      >
        <div
          role="row"
          className="grid grid-cols-[1.4fr_1fr_1fr_1.2fr_1.6fr_0.8fr] gap-2 border-b border-saw-grey-200 bg-saw-grey-50 px-4 py-2 text-small font-medium text-saw-grey-700"
        >
          <span role="columnheader">
            {t("dashboard.scans.column.started_at")}
          </span>
          <span role="columnheader">
            {t("dashboard.scans.column.account")}
          </span>
          <span role="columnheader">
            {t("dashboard.scans.column.status")}
          </span>
          <span role="columnheader">
            {t("dashboard.scans.column.severity")}
          </span>
          <span role="columnheader">
            {t("dashboard.scans.column.actions")}
          </span>
          <span role="columnheader" className="sr-only">
            id
          </span>
        </div>
        {scans.map((s) => {
          const counts = scanCounts[s.scan_id];
          return (
            <div
              role="row"
              key={s.scan_id}
              data-testid={`scan-row-${s.scan_id}`}
              className="grid grid-cols-[1.4fr_1fr_1fr_1.2fr_1.6fr_0.8fr] items-center gap-2 border-b border-saw-grey-100 px-4 py-3 last:border-b-0"
            >
              <span role="cell" className="text-body text-saw-grey-900">
                {formatDate(s.started_at)}
              </span>
              <span role="cell" className="text-small text-saw-grey-700">
                {showId(s.aws_account_id)}
              </span>
              <span role="cell">
                <ScanStatusBadge status={s.status} />
              </span>
              <span role="cell">
                <SeverityCounts counts={counts} />
              </span>
              <span role="cell" className="flex flex-wrap gap-2">
                {isTerminalScanStatus(s.status) ? (
                  <>
                    <Button
                      variant="secondary"
                      size="sm"
                      onClick={() => onOpenScan(s.scan_id)}
                      data-testid={`scan-open-${s.scan_id}`}
                    >
                      {t("dashboard.scans.open_scan")}
                    </Button>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => onDeleteScan(s)}
                      data-testid={`scan-delete-${s.scan_id}`}
                    >
                      {t("delete.scan.cta")}
                    </Button>
                  </>
                ) : (
                  <span className="text-small text-saw-grey-500">
                    {t("dashboard.scans.severity.unavailable")}
                  </span>
                )}
              </span>
              <span role="cell" className="sr-only">
                {s.scan_id}
              </span>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function ScanStatusBadge({ status }: { status: ScanStatus }) {
  const t = useT();
  const tone = {
    complete: "success",
    complete_with_warnings: "warning",
    failed: "danger",
    canceled: "neutral",
    pending: "info",
    assuming_role: "info",
    scanning: "info",
    parsing: "info",
  }[status] as "success" | "warning" | "danger" | "neutral" | "info";
  return <Badge tone={tone}>{t(`scanner.status.${status}`)}</Badge>;
}

function SeverityCounts({
  counts,
}: {
  counts: Record<Severity, number> | undefined;
}) {
  const t = useT();
  if (!counts) {
    return (
      <span className="text-small text-saw-grey-500">
        {t("dashboard.scans.severity.loading")}
      </span>
    );
  }
  const total = SEVERITY_ORDER.reduce((acc, s) => acc + (counts[s] ?? 0), 0);
  if (total === 0) {
    return (
      <span className="text-small text-saw-grey-600">
        {t("dashboard.scans.severity.zero")}
      </span>
    );
  }
  return (
    <span className="flex flex-wrap gap-1">
      {SEVERITY_ORDER.map((sev) =>
        (counts[sev] ?? 0) > 0 ? (
          <span
            key={sev}
            className="inline-flex items-center gap-1 rounded-full bg-saw-grey-100 px-2 py-0.5 text-small text-saw-grey-800"
            data-testid={`scan-sev-${sev}`}
            aria-label={`${t(`dashboard.severity.${sev}`)}: ${counts[sev]}`}
          >
            <SeverityBadge severity={sev} size="sm" iconOnly />
            {counts[sev]}
          </span>
        ) : null,
      )}
    </span>
  );
}

// ----- Helpers ------------------------------------------------------------

function countBySeverity(findings: Finding[]): Record<Severity, number> {
  const acc: Record<Severity, number> = {
    critical: 0,
    high: 0,
    medium: 0,
    low: 0,
    informational: 0,
  };
  for (const f of findings) {
    if (f.status === "open") acc[f.severity] += 1;
  }
  return acc;
}

function useChartSeriesFromScans(
  scans: ScanRecord[],
  scanCounts: Record<string, Record<Severity, number>>,
) {
  return useMemo(() => {
    const terminal = scans
      .filter((s) => isTerminalScanStatus(s.status))
      .slice()
      .sort(
        (a, b) =>
          new Date(a.started_at).getTime() - new Date(b.started_at).getTime(),
      );
    const colors: Record<Severity, string> = {
      critical: "#1F1F1F",
      high: "#D7263D",
      medium: "#F58A1F",
      low: "#E5B43A",
      informational: "#9CA3AF",
    };
    return SEVERITY_ORDER.map((sev) => ({
      id: sev,
      label: titleCase(sev),
      color: colors[sev],
      points: terminal.map((s, i) => ({
        x: i,
        y: scanCounts[s.scan_id]?.[sev] ?? 0,
        label: formatDate(s.started_at),
      })),
    }));
  }, [scans, scanCounts]);
}

function titleCase(s: string): string {
  return s.charAt(0).toUpperCase() + s.slice(1);
}

function formatDate(iso: string): string {
  try {
    const d = new Date(iso);
    return d.toLocaleString();
  } catch {
    return iso;
  }
}

function CopyDiagnostic({ info }: { info: string }) {
  const t = useT();
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      onClick={() => {
        void navigator.clipboard?.writeText(info).then(
          () => {
            setCopied(true);
            window.setTimeout(() => setCopied(false), 2000);
          },
          () => {
            // ignore — clipboard rejection is non-fatal
          },
        );
      }}
      className="ml-3 inline text-small text-saw-grey-700 underline underline-offset-2 hover:text-saw-grey-900"
      data-testid="dashboard-copy-diagnostic"
    >
      {copied
        ? t("dashboard.error.copy_diagnostic.copied")
        : t("dashboard.error.copy_diagnostic")}
    </button>
  );
}

// ----- Hard delete dialog (Contract 11C) ---------------------------------

function HardDeleteDialog({
  target,
  account,
  onCancel,
  onConfirm,
  showId,
}: {
  target: ScanRecord | null;
  account: Account | null;
  onCancel: () => void;
  onConfirm: (
    target: ScanRecord,
    confirmation: string,
    secureOverwrite: boolean,
  ) => Promise<HardDeleteSummary>;
  showId: (id: string) => string;
}) {
  const t = useT();
  const formatError = useIpcError();
  const [confirmation, setConfirmation] = useState("");
  const [secure, setSecure] = useState(false);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    if (!target) {
      setConfirmation("");
      setSecure(false);
      setErr(null);
    }
  }, [target]);

  if (!target) return null;

  const valid = confirmation === "DELETE" || confirmation === target.scan_id;

  async function submit() {
    if (!valid || !target) return;
    setBusy(true);
    setErr(null);
    try {
      await onConfirm(target, confirmation, secure);
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal
      open={!!target}
      onClose={onCancel}
      title={t("delete.scan.title")}
      footer={
        <>
          <Button variant="ghost" onClick={onCancel} disabled={busy}>
            {t("delete.scan.cancel")}
          </Button>
          <Button
            variant="primary"
            onClick={() => void submit()}
            disabled={!valid || busy}
            data-testid="hard-delete-confirm"
          >
            {busy ? t("delete.scan.busy") : t("delete.scan.confirm_cta")}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-3">
        <p>
          {t("delete.scan.explainer")
            .replace("{scan}", target.scan_id.slice(0, 12) + "…")
            .replace("{account}", account ? `${account.label} · ${showId(target.aws_account_id)}` : showId(target.aws_account_id))}
        </p>
        <p className="text-small text-saw-red">{t("delete.scan.warning")}</p>
        <label className="flex flex-col gap-1 text-small text-saw-grey-700">
          <span>{t("delete.scan.confirm_label")}</span>
          <input
            type="text"
            value={confirmation}
            onChange={(e) => setConfirmation(e.target.value)}
            placeholder={t("delete.scan.confirm_placeholder")}
            autoFocus
            className="rounded-card border border-saw-grey-200 bg-saw-white px-3 py-1.5 text-body text-saw-grey-900"
            data-testid="hard-delete-input"
          />
        </label>
        <Switch
          label={t("delete.scan.secure_overwrite_label")}
          description={t("delete.scan.secure_overwrite_hint")}
          checked={secure}
          onChange={setSecure}
        />
        {err ? (
          <p
            role="alert"
            className="rounded-card bg-saw-grey-100 px-3 py-2 text-small text-saw-red"
          >
            {err}
          </p>
        ) : null}
      </div>
    </Modal>
  );
}
