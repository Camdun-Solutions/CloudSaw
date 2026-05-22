// DriftView — cross-scan comparison for an account.
//
// Picks two terminal scans (baseline and target) and reports new / resolved
// / unchanged findings between them. Includes a count-over-time graph
// across all available scans (Contract 09 §Expected Output).

import { useEffect, useMemo, useState } from "react";

import {
  EmptyState,
  LineChart,
  SeverityBadge,
  Select,
} from "@/components";
import { useT } from "@/hooks/useT";
import { useIpcError } from "@/hooks/useIpcError";
import {
  ipc,
  isTerminalScanStatus,
  SEVERITY_ORDER,
  type Account,
  type Finding,
  type ScanRecord,
  type Severity,
} from "@/lib/ipc";

type Props = {
  account: Account;
  scans: ScanRecord[];
};

export default function DriftView({ account, scans }: Props) {
  const t = useT();
  const formatError = useIpcError();

  const terminal = useMemo(
    () =>
      scans
        .filter((s) => isTerminalScanStatus(s.status))
        .slice()
        .sort(
          (a, b) =>
            new Date(b.started_at).getTime() -
            new Date(a.started_at).getTime(),
        ),
    [scans],
  );

  const [baseId, setBaseId] = useState<string | null>(null);
  const [targetId, setTargetId] = useState<string | null>(null);
  const [baseFindings, setBaseFindings] = useState<Finding[] | null>(null);
  const [targetFindings, setTargetFindings] = useState<Finding[] | null>(null);
  const [allCounts, setAllCounts] = useState<
    Record<string, Record<Severity, number>>
  >({});
  const [error, setError] = useState<string | null>(null);

  // Default selection: target = most recent, base = second-most-recent.
  useEffect(() => {
    if (terminal.length >= 2) {
      setTargetId((cur) => cur ?? terminal[0].scan_id);
      setBaseId((cur) => cur ?? terminal[1].scan_id);
    } else if (terminal.length === 1) {
      setTargetId(terminal[0].scan_id);
      setBaseId(null);
    }
  }, [terminal]);

  // Per-scan finding lists for the two chosen scans.
  useEffect(() => {
    if (!baseId) {
      setBaseFindings(null);
      return;
    }
    let cancelled = false;
    setBaseFindings(null);
    ipc
      .findingsList(baseId)
      .then((list) => {
        if (!cancelled) setBaseFindings(list);
      })
      .catch((err) => {
        if (!cancelled) setError(formatError(err));
      });
    return () => {
      cancelled = true;
    };
  }, [baseId, formatError]);

  useEffect(() => {
    if (!targetId) {
      setTargetFindings(null);
      return;
    }
    let cancelled = false;
    setTargetFindings(null);
    ipc
      .findingsList(targetId)
      .then((list) => {
        if (!cancelled) setTargetFindings(list);
      })
      .catch((err) => {
        if (!cancelled) setError(formatError(err));
      });
    return () => {
      cancelled = true;
    };
  }, [targetId, formatError]);

  // Count-over-time chart: load summaries for all terminal scans.
  useEffect(() => {
    let cancelled = false;
    terminal.forEach((s) => {
      if (allCounts[s.scan_id]) return;
      ipc
        .findingsList(s.scan_id)
        .then((list) => {
          if (cancelled) return;
          const counts: Record<Severity, number> = {
            critical: 0,
            high: 0,
            medium: 0,
            low: 0,
            informational: 0,
          };
          for (const f of list) {
            if (f.status === "open") counts[f.severity] += 1;
          }
          setAllCounts((prev) => ({ ...prev, [s.scan_id]: counts }));
        })
        .catch(() => {
          /* best-effort */
        });
    });
    return () => {
      cancelled = true;
    };
  }, [terminal, allCounts]);

  const chartSeries = useMemo(() => {
    const ordered = terminal
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
      points: ordered.map((s, i) => ({
        x: i,
        y: allCounts[s.scan_id]?.[sev] ?? 0,
        label: new Date(s.started_at).toLocaleDateString(),
      })),
    }));
  }, [terminal, allCounts]);

  if (terminal.length < 2) {
    return (
      <EmptyState
        title={t("dashboard.drift.empty.title")}
        body={t("dashboard.drift.empty.body")}
      />
    );
  }

  if (baseId === targetId) {
    return (
      <div className="space-y-4">
        <DriftHeader
          account={account}
          terminal={terminal}
          baseId={baseId}
          targetId={targetId}
          setBaseId={setBaseId}
          setTargetId={setTargetId}
        />
        <EmptyState
          title={t("dashboard.drift.empty.title")}
          body={t("dashboard.drift.same_scan")}
        />
      </div>
    );
  }

  const diff = computeDiff(baseFindings, targetFindings);

  return (
    <div className="space-y-4">
      <DriftHeader
        account={account}
        terminal={terminal}
        baseId={baseId}
        targetId={targetId}
        setBaseId={setBaseId}
        setTargetId={setTargetId}
      />

      {error ? (
        <p
          role="alert"
          className="rounded-card border border-saw-red/40 bg-saw-red/5 px-4 py-2 text-body text-saw-grey-900"
        >
          {error}
        </p>
      ) : null}

      <LineChart
        ariaTitle={t("dashboard.drift.chart.title")}
        series={chartSeries}
      />

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-3">
        <DriftGroup
          title={t("dashboard.drift.section.new")}
          findings={diff.added}
          testIdSuffix="new"
        />
        <DriftGroup
          title={t("dashboard.drift.section.resolved")}
          findings={diff.removed}
          testIdSuffix="resolved"
        />
        <DriftGroup
          title={t("dashboard.drift.section.unchanged")}
          findings={diff.unchanged}
          testIdSuffix="unchanged"
        />
      </div>
    </div>
  );
}

type DriftHeaderProps = {
  account: Account;
  terminal: ScanRecord[];
  baseId: string | null;
  targetId: string | null;
  setBaseId: (id: string | null) => void;
  setTargetId: (id: string | null) => void;
};

function DriftHeader({
  account,
  terminal,
  baseId,
  targetId,
  setBaseId,
  setTargetId,
}: DriftHeaderProps) {
  const t = useT();
  const opts = terminal.map((s) => ({
    value: s.scan_id,
    label: `${new Date(s.started_at).toLocaleString()} (${s.scan_id.slice(0, 8)})`,
  }));
  return (
    <div className="space-y-3">
      <h2 className="text-h2 font-semibold">{t("dashboard.drift.title")}</h2>
      <p className="text-small text-saw-grey-600">
        {t("dashboard.drift.subtitle").replace("{account}", account.label)}
      </p>
      <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
        <Select<string>
          label={t("dashboard.drift.select.base")}
          value={baseId ?? ""}
          onChange={setBaseId}
          options={opts}
          data-testid="drift-select-base"
        />
        <Select<string>
          label={t("dashboard.drift.select.target")}
          value={targetId ?? ""}
          onChange={setTargetId}
          options={opts}
          data-testid="drift-select-target"
        />
      </div>
    </div>
  );
}

function DriftGroup({
  title,
  findings,
  testIdSuffix,
}: {
  title: string;
  findings: Finding[] | null;
  testIdSuffix: string;
}) {
  const t = useT();
  return (
    <section
      className="rounded-card border border-saw-grey-200 bg-saw-white p-4"
      data-testid={`drift-group-${testIdSuffix}`}
    >
      <header className="flex items-center justify-between">
        <h3 className="text-body font-semibold text-saw-grey-900">{title}</h3>
        <span
          className="text-small text-saw-grey-600"
          data-testid={`drift-count-${testIdSuffix}`}
        >
          {findings === null
            ? t("common.loading")
            : t("dashboard.drift.count").replace(
                "{count}",
                String(findings.length),
              )}
        </span>
      </header>
      {findings === null ? null : findings.length === 0 ? (
        <p className="mt-3 text-small text-saw-grey-600">
          {t("dashboard.drift.list.empty")}
        </p>
      ) : (
        <ul className="mt-3 space-y-2 max-h-80 overflow-y-auto pr-2">
          {findings.map((f) => (
            <li
              key={f.finding_id}
              className="flex items-center gap-2 text-small"
            >
              <SeverityBadge severity={f.severity} size="sm" iconOnly />
              <span className="truncate text-saw-grey-900">
                {f.dashboard_name || f.rule_key}
              </span>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

// ----- Diff helpers -------------------------------------------------------

function computeDiff(
  base: Finding[] | null,
  target: Finding[] | null,
): { added: Finding[] | null; removed: Finding[] | null; unchanged: Finding[] | null } {
  if (!base || !target) {
    return { added: null, removed: null, unchanged: null };
  }
  const baseIds = new Set(base.filter((f) => f.status === "open").map((f) => f.rule_key));
  const targetIds = new Set(
    target.filter((f) => f.status === "open").map((f) => f.rule_key),
  );
  const added = target.filter(
    (f) => f.status === "open" && !baseIds.has(f.rule_key),
  );
  const removed = base.filter(
    (f) => f.status === "open" && !targetIds.has(f.rule_key),
  );
  const unchanged = target.filter(
    (f) => f.status === "open" && baseIds.has(f.rule_key),
  );
  return { added, removed, unchanged };
}

function titleCase(s: string): string {
  return s.charAt(0).toUpperCase() + s.slice(1);
}
