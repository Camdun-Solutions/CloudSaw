// Findings — PR #51. Promotes findings from a Dashboard sub-tab to
// a first-class top-level route. The TopNav "Findings" button lands
// here directly (App.tsx route handler).
//
// Surface vs the legacy FindingsView.tsx (Dashboard sub-tab):
//   - Account auto-pick (active account, falling back to first
//     configured) with an inline selector so the user can switch
//     without leaving the page.
//   - Scan selector dropdown — defaults to the latest scan;
//     listed in date-desc order via `ipc.scannerListRecent`.
//   - Per-service collapsible <details> groups instead of the
//     split-pane flat list. Each finding row inside a group
//     carries a severity-colored left border (critical/high →
//     saw-red, medium → saw-gold, low/info → saw-grey-400,
//     resolved → saw-green per PR #51's new tailwind token).
//   - Click a finding to expand inline: renders the existing
//     FindingDetailPanel (re-used as-is) below the row — no
//     separate right pane. AI suggestion lives in that panel
//     and remains modal-flow for now (a focused follow-up PR
//     can flatten the AI hop if the user requests).
//   - Filters: severity / service / status / search query.
//     Search matches dashboard_name + rule_key + description
//     case-insensitively.
//
// What this PR DEFERS to follow-ups:
//   - Inline (non-modal) AI suggestion confirm flow.
//   - Remediation variant tabs (Terraform / AWS CLI /
//     CloudFormation). The data is in KB articles already.
//
// FindingsView.tsx (the legacy Dashboard sub-tab) is kept
// intact so any pre-existing UI surface that still routes to
// Dashboard.tsx with initialTab="findings" continues to work.

import { useEffect, useMemo, useState } from "react";

import {
  BackBreadcrumb,
  Badge,
  EmptyState,
  Logo,
  Select,
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
  type FindingStatus,
  type ScanRecord,
  type Severity,
} from "@/lib/ipc";
// Re-uses the existing detail panel (KB articles, AI suggestion,
// GitHub ticket linking, resources, control mappings) verbatim so
// PR #51 doesn't fork that surface. It's defined inline in
// FindingsView.tsx today — exported there for this consumer.
import { FindingDetailPanel } from "@/routes/dashboard/FindingsView";

type Props = {
  onBack: () => void;
};

type SevFilter = "any" | Severity;
type StatusFilter = "any" | FindingStatus;

/** Border color for the severity-coded finding-row left edge.
 *  Resolved findings win regardless of severity — they get the
 *  "well-configured" green border. Open findings get the
 *  severity-ranked colors per PR #51's spec. */
function severityBorder(sev: Severity, status: FindingStatus): string {
  if (status === "resolved") return "border-l-saw-green";
  switch (sev) {
    case "critical":
    case "high":
      return "border-l-saw-red";
    case "medium":
      return "border-l-saw-gold";
    case "low":
    case "informational":
      return "border-l-saw-grey-400";
  }
}

/** Worst-severity-first ordering for the per-service group
 *  header badges and for picking a service group's "open by
 *  default" gating (groups containing a critical/high finding
 *  open automatically; others default closed). */
function rankSeverity(s: Severity): number {
  return SEVERITY_ORDER.indexOf(s);
}

export default function Findings({ onBack }: Props) {
  const t = useT();
  const formatError = useIpcError();

  // Account selection + scan selection.
  const [accounts, setAccounts] = useState<Account[] | null>(null);
  const [accountId, setAccountId] = useState<string | null>(null);
  const [scans, setScans] = useState<ScanRecord[] | null>(null);
  const [scanId, setScanId] = useState<string | null>(null);

  // Findings + filter state.
  const [findings, setFindings] = useState<Finding[] | null>(null);
  const [sev, setSev] = useState<SevFilter>("any");
  const [service, setService] = useState<string>("any");
  const [status, setStatus] = useState<StatusFilter>("any");
  const [query, setQuery] = useState("");
  const [expandedId, setExpandedId] = useState<string | null>(null);

  const [error, setError] = useState<string | null>(null);

  // 1. Bootstrap: load accounts + auto-pick active (or first).
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const [list, active] = await Promise.all([
          ipc.accountsList(),
          ipc.accountsGetActive(),
        ]);
        if (cancelled) return;
        setAccounts(list);
        const pick = active ?? list[0]?.aws_account_id ?? null;
        setAccountId(pick);
      } catch (err) {
        if (!cancelled) setError(formatError(err));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [formatError]);

  // 2. When the account changes, load its scans and auto-pick
  //    the latest one. Resets findings/expansion in the process.
  useEffect(() => {
    if (!accountId) {
      setScans([]);
      setScanId(null);
      return;
    }
    let cancelled = false;
    setScans(null);
    setScanId(null);
    setFindings(null);
    setExpandedId(null);
    (async () => {
      try {
        const list = await ipc.scannerListRecent(accountId, 20);
        if (cancelled) return;
        // Sort newest first; pick the most recent terminal scan
        // with output as the default.
        const sorted = [...list].sort((a, b) => {
          return (
            new Date(b.started_at).getTime() -
            new Date(a.started_at).getTime()
          );
        });
        setScans(sorted);
        const latest = sorted.find(
          (s) =>
            (s.status === "complete" ||
              s.status === "complete_with_warnings") &&
            s.raw_output_path !== null,
        );
        setScanId(latest?.scan_id ?? sorted[0]?.scan_id ?? null);
      } catch (err) {
        if (!cancelled) setError(formatError(err));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [accountId, formatError]);

  // 3. When scan changes, fetch its findings.
  useEffect(() => {
    if (!scanId) {
      setFindings([]);
      return;
    }
    let cancelled = false;
    setFindings(null);
    setExpandedId(null);
    (async () => {
      try {
        const list = await ipc.findingsList(scanId);
        if (!cancelled) setFindings(list);
      } catch (err) {
        if (!cancelled) {
          setError(formatError(err));
          setFindings([]);
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [scanId, formatError]);

  // --- Derived state -----------------------------------------------------

  const filtered = useMemo(() => {
    if (!findings) return [] as Finding[];
    const q = query.trim().toLowerCase();
    return findings.filter((f) => {
      if (sev !== "any" && f.severity !== sev) return false;
      if (status !== "any" && f.status !== status) return false;
      if (service !== "any" && f.service !== service) return false;
      if (q) {
        const hay = `${f.dashboard_name ?? ""} ${f.rule_key} ${f.description}`
          .toLowerCase();
        if (!hay.includes(q)) return false;
      }
      return true;
    });
  }, [findings, sev, service, status, query]);

  /** Group filtered findings by service, sorted alphabetically.
   *  Each group carries its worst severity (used to render the
   *  group header badge + decide default-open state). */
  const grouped = useMemo(() => {
    type Group = {
      service: string;
      worst: Severity;
      openByDefault: boolean;
      items: Finding[];
    };
    const map = new Map<string, Group>();
    filtered.forEach((f) => {
      const key = f.service || "other";
      const existing = map.get(key);
      if (!existing) {
        map.set(key, {
          service: key,
          worst: f.severity,
          openByDefault:
            f.severity === "critical" || f.severity === "high",
          items: [f],
        });
      } else {
        existing.items.push(f);
        if (rankSeverity(f.severity) < rankSeverity(existing.worst)) {
          existing.worst = f.severity;
        }
        if (f.severity === "critical" || f.severity === "high") {
          existing.openByDefault = true;
        }
      }
    });
    return Array.from(map.values()).sort((a, b) => {
      // Worst-severity groups first; within same severity, alpha by service.
      const sevDiff = rankSeverity(a.worst) - rankSeverity(b.worst);
      return sevDiff !== 0 ? sevDiff : a.service.localeCompare(b.service);
    });
  }, [filtered]);

  const serviceOptions = useMemo(() => {
    const set = new Set<string>();
    (findings ?? []).forEach((f) => set.add(f.service || "other"));
    const list = Array.from(set).sort((a, b) => a.localeCompare(b));
    return [
      { value: "any", label: t("dashboard.findings.filter.all") },
      ...list.map((s) => ({ value: s, label: s })),
    ];
  }, [findings, t]);

  const sevOptions: { value: SevFilter; label: string }[] = [
    { value: "any", label: t("dashboard.findings.filter.all") },
    { value: "critical", label: t("dashboard.severity.critical") },
    { value: "high", label: t("dashboard.severity.high") },
    { value: "medium", label: t("dashboard.severity.medium") },
    { value: "low", label: t("dashboard.severity.low") },
    {
      value: "informational",
      label: t("dashboard.severity.informational"),
    },
  ];

  const statusOptions: { value: StatusFilter; label: string }[] = [
    { value: "any", label: t("dashboard.findings.filter.all") },
    { value: "open", label: t("dashboard.status.open") },
    { value: "resolved", label: t("dashboard.status.resolved") },
  ];

  const accountOptions = (accounts ?? []).map((a) => ({
    value: a.aws_account_id,
    label: `${a.label} (${maskAccountId(a.aws_account_id)})`,
  }));

  const scanOptions = (scans ?? []).map((s) => ({
    value: s.scan_id,
    label: `${formatScanTimestamp(s.started_at)} · ${s.status}`,
  }));

  // --- Render ------------------------------------------------------------

  return (
    <main className="min-h-full bg-saw-grey-50 dark:bg-saw-black text-saw-grey-900 dark:text-saw-beige">
      <header className="border-b border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-8 py-5">
        <BackBreadcrumb
          destination={t("nav.dashboard")}
          onBack={onBack}
          data-testid="findings-back"
        />
        <div className="mt-2 flex items-center gap-3">
          <Logo size="sm" />
          <div className="flex flex-col">
            <h1 className="text-h2 font-semibold tracking-tight">
              {t("findings.page.title")}
            </h1>
            <p className="text-small text-saw-grey-500 dark:text-saw-grey-400">
              {t("findings.page.subtitle")}
            </p>
          </div>
        </div>
      </header>

      <section className="mx-auto max-w-7xl px-8 py-8">
        {accounts === null ? (
          <p className="text-body text-saw-grey-600 dark:text-saw-grey-400">{t("common.loading")}</p>
        ) : accounts.length === 0 ? (
          <EmptyState
            title={t("findings.empty.no_accounts.title")}
            body={t("findings.empty.no_accounts.body")}
          />
        ) : (
          <>
            {/* Account + scan pickers. Side-by-side so the user
                can sweep across accounts and scans without
                touching their finger to the scroll wheel. */}
            <div className="grid gap-3 lg:grid-cols-2">
              <Select<string>
                label={t("findings.account_label")}
                value={accountId ?? ""}
                options={accountOptions}
                onChange={(v) => setAccountId(v || null)}
                data-testid="findings-account-select"
              />
              <Select<string>
                label={t("findings.scan_label")}
                value={scanId ?? ""}
                options={scanOptions}
                onChange={(v) => setScanId(v || null)}
                data-testid="findings-scan-select"
              />
            </div>

            {/* Filter bar — severity / service / status / search. */}
            <div className="mt-4 grid gap-3 lg:grid-cols-4">
              <Select<SevFilter>
                label={t("dashboard.findings.filter.severity")}
                value={sev}
                options={sevOptions}
                onChange={setSev}
                data-testid="findings-filter-sev"
              />
              <Select<string>
                label={t("dashboard.findings.filter.service")}
                value={service}
                options={serviceOptions}
                onChange={setService}
                data-testid="findings-filter-service"
              />
              <Select<StatusFilter>
                label={t("dashboard.findings.filter.status")}
                value={status}
                options={statusOptions}
                onChange={setStatus}
                data-testid="findings-filter-status"
              />
              <label className="flex flex-col gap-1 text-small text-saw-grey-700 dark:text-saw-grey-300">
                <span>{t("findings.filter.search")}</span>
                <input
                  type="search"
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  placeholder={t("findings.filter.search_placeholder")}
                  className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-3 py-1.5 text-body text-saw-grey-900 dark:text-saw-beige"
                  data-testid="findings-filter-search"
                />
              </label>
            </div>

            {error ? (
              <p
                role="alert"
                className="mt-4 rounded-card border border-saw-red/40 bg-saw-red/5 px-4 py-3 text-body text-saw-grey-900 dark:text-saw-beige"
                data-testid="findings-error"
              >
                {error}
              </p>
            ) : null}

            {/* Per-service collapsible groups. */}
            <div className="mt-6 flex flex-col gap-3" data-testid="findings-groups">
              {findings === null ? (
                <p className="text-body text-saw-grey-600">
                  {t("common.loading")}
                </p>
              ) : grouped.length === 0 ? (
                <EmptyState
                  title={t("findings.empty.no_findings.title")}
                  body={t("findings.empty.no_findings.body")}
                />
              ) : (
                grouped.map((g) => (
                  <ServiceGroup
                    key={g.service}
                    service={g.service}
                    worst={g.worst}
                    items={g.items}
                    openByDefault={g.openByDefault}
                    expandedId={expandedId}
                    onToggleFinding={(id) =>
                      setExpandedId((cur) => (cur === id ? null : id))
                    }
                  />
                ))
              )}
            </div>
          </>
        )}
      </section>
    </main>
  );
}

// --- Subcomponents -----------------------------------------------------

function ServiceGroup({
  service,
  worst,
  items,
  openByDefault,
  expandedId,
  onToggleFinding,
}: {
  service: string;
  worst: Severity;
  items: Finding[];
  openByDefault: boolean;
  expandedId: string | null;
  onToggleFinding: (id: string) => void;
}) {
  const t = useT();
  return (
    <details
      open={openByDefault}
      className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark overflow-hidden"
      data-testid="findings-service-group"
    >
      <summary className="flex cursor-pointer items-center gap-3 px-4 py-3 hover:bg-saw-grey-50 dark:hover:bg-saw-grey-800">
        <span className="text-h3 font-semibold text-saw-grey-900 dark:text-saw-beige">
          {service}
        </span>
        <Badge tone="neutral">
          {t("findings.group.count").replace("{count}", String(items.length))}
        </Badge>
        <SeverityBadge severity={worst} />
        <span className="ml-auto text-xs text-saw-grey-500 dark:text-saw-grey-400">
          {t("findings.group.toggle_hint")}
        </span>
      </summary>
      <ul className="divide-y divide-saw-grey-100 dark:divide-saw-grey-800">
        {items.map((f) => (
          <FindingRow
            key={f.finding_id}
            finding={f}
            expanded={expandedId === f.finding_id}
            onToggle={() => onToggleFinding(f.finding_id)}
          />
        ))}
      </ul>
    </details>
  );
}

function FindingRow({
  finding,
  expanded,
  onToggle,
}: {
  finding: Finding;
  expanded: boolean;
  onToggle: () => void;
}) {
  const t = useT();
  const borderClass = severityBorder(finding.severity, finding.status);
  return (
    <li
      className={`border-l-4 ${borderClass} bg-saw-white dark:bg-saw-grey-dark`}
      data-testid="finding-row"
    >
      <button
        type="button"
        onClick={onToggle}
        aria-expanded={expanded}
        className="flex w-full items-start gap-3 px-4 py-3 text-left hover:bg-saw-grey-50 dark:hover:bg-saw-grey-800"
      >
        <SeverityBadge severity={finding.severity} />
        <div className="flex min-w-0 flex-1 flex-col">
          <span className="text-small font-medium text-saw-grey-900 dark:text-saw-beige truncate">
            {finding.dashboard_name ?? finding.rule_key}
          </span>
          <span className="text-xs text-saw-grey-500 dark:text-saw-grey-400">
            {finding.flagged_items}/{finding.checked_items}{" "}
            {t("findings.row.affected_label")}
          </span>
        </div>
        <Badge tone={finding.status === "resolved" ? "success" : "neutral"}>
          {t(`dashboard.status.${finding.status}`)}
        </Badge>
      </button>
      {expanded ? (
        <div className="border-t border-saw-grey-100 dark:border-saw-grey-800 bg-saw-grey-50 dark:bg-saw-black px-4 py-4">
          {/* FindingDetailPanel re-used as-is from the legacy
              FindingsView. Renders KB article, AI suggestion
              button, GitHub ticket linking, resources, control
              mappings — all the existing logic. */}
          <FindingDetailPanel findingId={finding.finding_id} />
        </div>
      ) : null}
    </li>
  );
}

function formatScanTimestamp(iso: string): string {
  try {
    return new Date(iso).toLocaleString();
  } catch {
    return iso;
  }
}
