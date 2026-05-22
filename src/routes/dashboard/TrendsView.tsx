// TrendsView — severity counts over time, mean-time-to-remediation per
// severity, and a per-finding remediation timeline (Contract 09).
//
// All data comes from the indefinitely-retained findings metadata: each
// finding row carries first_seen_at, last_seen_at, and resolved_at — so we
// can compute MTTR and the per-finding timeline without storing any new
// shape.

import { useEffect, useMemo, useState } from "react";

import {
  EmptyState,
  LineChart,
  SeverityBadge,
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

const MS_PER_DAY = 1000 * 60 * 60 * 24;

export default function TrendsView({ account, scans }: Props) {
  const t = useT();
  const formatError = useIpcError();
  const [scanFindings, setScanFindings] = useState<Record<string, Finding[]>>(
    {},
  );
  const [error, setError] = useState<string | null>(null);

  const terminal = useMemo(
    () =>
      scans
        .filter((s) => isTerminalScanStatus(s.status))
        .slice()
        .sort(
          (a, b) =>
            new Date(a.started_at).getTime() -
            new Date(b.started_at).getTime(),
        ),
    [scans],
  );

  useEffect(() => {
    let cancelled = false;
    terminal.forEach((s) => {
      if (scanFindings[s.scan_id]) return;
      ipc
        .findingsList(s.scan_id)
        .then((list) => {
          if (!cancelled) {
            setScanFindings((prev) => ({ ...prev, [s.scan_id]: list }));
          }
        })
        .catch((err) => {
          if (!cancelled) setError(formatError(err));
        });
    });
    return () => {
      cancelled = true;
    };
  }, [terminal, scanFindings, formatError]);

  // Collect a unique-finding set keyed by rule_key, picking the latest
  // observation. This lets us compute remediation timelines without
  // double-counting reappearances.
  const uniqueFindings = useMemo(() => {
    const map = new Map<string, Finding>();
    Object.values(scanFindings)
      .flat()
      .forEach((f) => {
        const cur = map.get(f.rule_key);
        if (!cur || new Date(f.last_seen_at) > new Date(cur.last_seen_at)) {
          map.set(f.rule_key, f);
        }
      });
    return Array.from(map.values());
  }, [scanFindings]);

  const mttrBySeverity = useMemo(() => computeMTTR(uniqueFindings), [
    uniqueFindings,
  ]);

  const severityChart = useMemo(() => {
    const colors: Record<Severity, string> = {
      critical: "#1F1F1F",
      high: "#D7263D",
      medium: "#F58A1F",
      low: "#E5B43A",
      informational: "#9CA3AF",
    };
    return SEVERITY_ORDER.map((sev) => ({
      id: sev,
      label: t(`dashboard.severity.${sev}`),
      color: colors[sev],
      points: terminal.map((s, i) => {
        const list = scanFindings[s.scan_id] ?? [];
        const count = list.filter(
          (f) => f.status === "open" && f.severity === sev,
        ).length;
        return {
          x: i,
          y: count,
          label: new Date(s.started_at).toLocaleDateString(),
        };
      }),
    }));
  }, [terminal, scanFindings, t]);

  const timelineEntries = useMemo(
    () =>
      uniqueFindings
        .slice()
        .sort(
          (a, b) =>
            new Date(b.last_seen_at).getTime() -
            new Date(a.last_seen_at).getTime(),
        )
        .slice(0, 20),
    [uniqueFindings],
  );

  if (terminal.length === 0) {
    return (
      <EmptyState
        title={t("dashboard.trends.empty.title")}
        body={t("dashboard.trends.empty.body")}
      />
    );
  }

  return (
    <div className="space-y-4">
      <h2 className="text-h2 font-semibold">
        {t("dashboard.trends.title")}
      </h2>
      <p className="text-small text-saw-grey-600">
        {t("dashboard.trends.subtitle").replace("{account}", account.label)}
      </p>

      {error ? (
        <p
          role="alert"
          className="rounded-card border border-saw-red/40 bg-saw-red/5 px-4 py-2 text-body text-saw-grey-900"
        >
          {error}
        </p>
      ) : null}

      <LineChart
        ariaTitle={t("dashboard.trends.chart.severity_title")}
        series={severityChart}
      />

      <section
        className="rounded-card border border-saw-grey-200 bg-saw-white p-4"
        data-testid="trends-mttr"
      >
        <h3 className="text-body font-semibold text-saw-grey-900">
          {t("dashboard.trends.mttr.title")}
        </h3>
        <p className="text-small text-saw-grey-600">
          {t("dashboard.trends.mttr.subtitle")}
        </p>
        <ul className="mt-3 grid grid-cols-1 gap-2 md:grid-cols-5">
          {SEVERITY_ORDER.map((sev) => {
            const v = mttrBySeverity[sev];
            return (
              <li
                key={sev}
                className="flex flex-col items-start gap-1 rounded-card bg-saw-grey-50 p-3"
                data-testid={`mttr-${sev}`}
              >
                <SeverityBadge severity={sev} size="sm" />
                <span className="text-body font-semibold text-saw-grey-900">
                  {v.count === 0
                    ? t("dashboard.trends.mttr.unresolved")
                    : t("dashboard.trends.mttr.value").replace(
                        "{days}",
                        v.days.toFixed(1),
                      )}
                </span>
              </li>
            );
          })}
        </ul>
      </section>

      <section
        className="rounded-card border border-saw-grey-200 bg-saw-white p-4"
        data-testid="trends-timeline"
      >
        <h3 className="text-body font-semibold text-saw-grey-900">
          {t("dashboard.trends.timeline.title")}
        </h3>
        <p className="text-small text-saw-grey-600">
          {t("dashboard.trends.timeline.subtitle")}
        </p>
        {timelineEntries.length === 0 ? (
          <p className="mt-3 text-small text-saw-grey-600">
            {t("dashboard.trends.empty.body")}
          </p>
        ) : (
          <ul className="mt-3 space-y-2">
            {timelineEntries.map((f) => (
              <li
                key={f.finding_id}
                className="flex items-center gap-3 rounded-card bg-saw-grey-50 px-3 py-2"
                data-testid={`timeline-${f.finding_id}`}
              >
                <SeverityBadge severity={f.severity} size="sm" iconOnly />
                <span className="flex-1 min-w-0">
                  <span className="block truncate text-body text-saw-grey-900">
                    {f.dashboard_name || f.rule_key}
                  </span>
                  <span className="block text-small text-saw-grey-600">
                    {formatTimelineSpan(f, t)}
                  </span>
                </span>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}

// ----- Stats --------------------------------------------------------------

function computeMTTR(
  findings: Finding[],
): Record<Severity, { count: number; days: number }> {
  const acc: Record<Severity, { count: number; total: number }> = {
    critical: { count: 0, total: 0 },
    high: { count: 0, total: 0 },
    medium: { count: 0, total: 0 },
    low: { count: 0, total: 0 },
    informational: { count: 0, total: 0 },
  };
  for (const f of findings) {
    if (f.status !== "resolved" || !f.resolved_at) continue;
    const ms =
      new Date(f.resolved_at).getTime() -
      new Date(f.first_seen_at).getTime();
    if (Number.isNaN(ms) || ms < 0) continue;
    acc[f.severity].count += 1;
    acc[f.severity].total += ms;
  }
  const out = {} as Record<Severity, { count: number; days: number }>;
  for (const sev of SEVERITY_ORDER) {
    const v = acc[sev];
    out[sev] =
      v.count === 0
        ? { count: 0, days: 0 }
        : { count: v.count, days: v.total / v.count / MS_PER_DAY };
  }
  return out;
}

function formatTimelineSpan(f: Finding, t: (k: string) => string): string {
  const first = new Date(f.first_seen_at).getTime();
  if (f.status === "resolved" && f.resolved_at) {
    const ms = new Date(f.resolved_at).getTime() - first;
    const days = Math.max(0, ms / MS_PER_DAY);
    return t("dashboard.trends.timeline.status.resolved").replace(
      "{days}",
      days.toFixed(1),
    );
  }
  const ms = Date.now() - first;
  const days = Math.max(0, ms / MS_PER_DAY);
  return t("dashboard.trends.timeline.status.open").replace(
    "{days}",
    days.toFixed(1),
  );
}
