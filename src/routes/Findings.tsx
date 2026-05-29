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
  Badge,
  Button,
  Drawer,
  EmptyState,
  Logo,
  Select,
  SeverityBadge,
} from "@/components";
import { useScanModal } from "@/contexts/ScanModalContext";
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

type Props = Record<string, never>;

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

export default function Findings(_props: Props) {
  const t = useT();
  const formatError = useIpcError();
  // PR #67: empty-state "Scan now" CTA opens the global scan modal,
  // which already routes to Settings → Accounts when no accounts or
  // role are configured (PR #63).
  const scanModal = useScanModal();

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
  // PR #80: clicking a finding row used to expand a detail panel
  // INLINE beneath the row (`expandedId`). Switched to a right-side
  // drawer so the user's per-service browse list stays intact while
  // they read the detail. State semantics changed: `selectedFinding`
  // is null when the drawer is closed, otherwise carries the row's
  // id (and we look up the service tag from `findings` so the drawer
  // header can name the parent context).
  const [selectedFindingId, setSelectedFindingId] = useState<string | null>(
    null,
  );

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
    setSelectedFindingId(null);
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
    setSelectedFindingId(null);
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
      {/* PR #75: sticky-top page header so the title bar stays
          visible while body content scrolls underneath. z-20 sits
          below the floating TopNav chip (z-30). */}
      <header className="sticky top-0 z-20 border-b border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-8 py-5">
        {/* PR #66: BackBreadcrumb removed — TopNav already exposes
            Dashboard/Findings/Settings buttons, so the per-page back
            arrow was redundant. */}
        <div className="flex items-center gap-3">
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
        {(() => {
          // PR #67: Findings empty-state restructure.
          //
          // "Universe-empty" means the user has nothing to show yet:
          // no accounts, OR no scans across any account, OR no
          // findings on the selected scan. In all three cases we
          // collapse to a single card with the "Scan now" CTA —
          // filters/pickers add no value with nothing to filter.
          //
          // While the initial loads are in flight (accounts === null,
          // or scans still null after accounts arrived) the page
          // renders silently — only the actual findings-fetch step
          // shows a "Loading…" inline indicator.
          const isInitialLoading =
            accounts === null || (accounts.length > 0 && scans === null);
          const universeEmpty =
            !isInitialLoading &&
            (accounts!.length === 0 ||
              (scans !== null && scans.length === 0) ||
              (findings !== null && findings.length === 0));

          if (isInitialLoading) {
            // Silent — only show a message when we're truly waiting
            // on findings, not on the initial accounts/scans round-
            // trip.
            return null;
          }

          if (universeEmpty) {
            return (
              <EmptyState
                title={t("findings.empty.run_scan.title")}
                body={t("findings.empty.run_scan.body")}
                action={
                  <Button
                    variant="primary"
                    onClick={() => scanModal.open()}
                    data-testid="findings-empty-scan-cta"
                  >
                    {t("scanner.scan.cta")}
                  </Button>
                }
              />
            );
          }

          return (
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
                      onSelectFinding={setSelectedFindingId}
                    />
                  ))
                )}
              </div>
            </>
          );
        })()}
      </section>

      {/* PR #80 — right-side drawer carries the detail panel for the
          finding the user clicked. The drawer mounts once at the
          route root so navigation back to it (via the service-group
          tab list) re-uses the same instance and doesn't re-fire
          the IPC fetches in `FindingDetailPanel`. */}
      <FindingDrawer
        findingId={selectedFindingId}
        findings={findings ?? []}
        onClose={() => setSelectedFindingId(null)}
      />
    </main>
  );
}

// --- Subcomponents -----------------------------------------------------

// PR #80 — severity tabs inside an expanded service group. The
// previous layout dumped every finding for the service into a flat
// `<ul>` ordered by severity. With a hundred findings per service
// (common on AWS) the user had to scroll past every critical to
// reach a high. Tabs let them jump straight to the band they care
// about; the default is the worst band present, so a service whose
// top severity is high doesn't render a useless Critical tab.
const TAB_SEVERITIES: Severity[] = [
  "critical",
  "high",
  "medium",
  "low",
  "informational",
];

function ServiceGroup({
  service,
  worst,
  items,
  openByDefault,
  onSelectFinding,
}: {
  service: string;
  worst: Severity;
  items: Finding[];
  openByDefault: boolean;
  onSelectFinding: (id: string) => void;
}) {
  const t = useT();

  // Bucket items by severity once per render — the tab strip and
  // the row list both read from this.
  const buckets = useMemo(() => {
    const out: Record<Severity, Finding[]> = {
      critical: [],
      high: [],
      medium: [],
      low: [],
      informational: [],
    };
    for (const f of items) out[f.severity].push(f);
    return out;
  }, [items]);

  // Only render tabs for severities that actually have findings in
  // this service. The first non-empty bucket (in severity order) is
  // the default — that's also `worst` by construction, but
  // computing locally keeps the component self-contained.
  const presentTabs = TAB_SEVERITIES.filter((s) => buckets[s].length > 0);
  const [active, setActive] = useState<Severity>(worst);

  // If the active tab no longer has items (e.g. an external filter
  // narrowed the set), snap to the worst remaining tab. Re-runs
  // whenever the bucket shape changes.
  useEffect(() => {
    if (presentTabs.length === 0) return;
    if (!presentTabs.includes(active)) {
      setActive(presentTabs[0]);
    }
  }, [presentTabs, active]);

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

      {/* Severity tab strip. Each tab is labeled with its severity
          word + count. Only present-tabs render so the strip never
          shows an empty Critical chip for a service whose worst is
          high. */}
      <div
        role="tablist"
        aria-label={t("findings.group.severity_tabs_aria")}
        data-testid="findings-severity-tabs"
        className="flex flex-wrap gap-1 border-b border-saw-grey-100 dark:border-saw-grey-800 bg-saw-grey-50 dark:bg-saw-black px-3 py-2"
      >
        {presentTabs.map((s) => {
          const isActive = s === active;
          return (
            <button
              key={s}
              type="button"
              role="tab"
              aria-selected={isActive}
              onClick={() => setActive(s)}
              data-testid={`findings-severity-tab-${s}`}
              className={[
                "inline-flex items-center gap-2 rounded-full px-3 py-1 text-small font-medium transition-colors",
                isActive
                  ? "bg-saw-white dark:bg-saw-grey-dark text-saw-grey-900 dark:text-saw-beige shadow-sm ring-1 ring-saw-grey-200 dark:ring-saw-grey-700"
                  : "text-saw-grey-600 dark:text-saw-grey-300 hover:bg-saw-white/60 dark:hover:bg-saw-grey-800",
                "focus:outline-none focus-visible:ring-2 focus-visible:ring-saw-orange",
              ].join(" ")}
            >
              <SeverityBadge severity={s} iconOnly />
              <span>{t(`dashboard.severity.${s}`)}</span>
              <span className="text-xs text-saw-grey-500 dark:text-saw-grey-400">
                {buckets[s].length}
              </span>
            </button>
          );
        })}
      </div>

      <ul
        role="tabpanel"
        aria-labelledby={`findings-severity-tab-${active}`}
        className="divide-y divide-saw-grey-100 dark:divide-saw-grey-800"
      >
        {buckets[active].map((f) => (
          <FindingRow
            key={f.finding_id}
            finding={f}
            onSelect={() => onSelectFinding(f.finding_id)}
          />
        ))}
      </ul>
    </details>
  );
}

function FindingRow({
  finding,
  onSelect,
}: {
  finding: Finding;
  onSelect: () => void;
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
        onClick={onSelect}
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
    </li>
  );
}

/** PR #80 — drawer host. Pulls the selected finding's name +
 *  service out of `findings` so the header can name the parent
 *  context; the actual content is the shared `FindingDetailPanel`
 *  that the legacy inline-expand layout used. */
function FindingDrawer({
  findingId,
  findings,
  onClose,
}: {
  findingId: string | null;
  findings: Finding[];
  onClose: () => void;
}) {
  const t = useT();
  const selected = findingId
    ? findings.find((f) => f.finding_id === findingId)
    : null;
  return (
    <Drawer
      open={!!findingId}
      onClose={onClose}
      title={selected?.dashboard_name ?? selected?.rule_key ?? t("findings.drawer.empty_title")}
      subtitle={selected?.service}
      size="lg"
      data-testid="findings-drawer"
    >
      {findingId ? <FindingDetailPanel findingId={findingId} /> : null}
    </Drawer>
  );
}

function formatScanTimestamp(iso: string): string {
  try {
    return new Date(iso).toLocaleString();
  } catch {
    return iso;
  }
}
