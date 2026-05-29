// Activity log — Contract 11A.
//
// PR #67 restructure:
//   - The component now owns its OWN section card (h2 + subtitle +
//     Export button at top-right). Settings.tsx renders
//     `<ActivityLog />` directly without an outer wrapper.
//   - Clear View + Refresh buttons removed (refresh runs on mount +
//     after any reload-relevant state change; clear-view added no
//     real value).
//   - New pagination footer: a "Showing X of Y" counter + a
//     rows-per-page selector (10 default, 20 / 50 / 100). Filtering
//     happens client-side because all rows already arrived inside
//     the 500-row reload cap.

import { useCallback, useEffect, useState } from "react";

import { Button, EmptyState, Select } from "@/components";
import { useT } from "@/hooks/useT";
import { useIpcError } from "@/hooks/useIpcError";
import { ipc, type EventKind, type EventLogEntry } from "@/lib/ipc";

const KIND_OPTIONS: ReadonlyArray<{ value: EventKind | "all"; key: string }> = [
  { value: "all", key: "eventlog.filter.all" },
  { value: "scan_completed", key: "eventlog.kind.scan_completed" },
  { value: "scan_failed", key: "eventlog.kind.scan_failed" },
  { value: "scan_canceled", key: "eventlog.kind.scan_canceled" },
  { value: "scheduled_scan_fired", key: "eventlog.kind.scheduled_scan_fired" },
  { value: "scheduled_scan_skipped", key: "eventlog.kind.scheduled_scan_skipped" },
  { value: "github_ticket_created", key: "eventlog.kind.github_ticket_created" },
  { value: "master_password_changed", key: "eventlog.kind.master_password_changed" },
  { value: "master_password_reset", key: "eventlog.kind.master_password_reset" },
  { value: "account_added", key: "eventlog.kind.account_added" },
  { value: "account_removed", key: "eventlog.kind.account_removed" },
  { value: "scan_deleted", key: "eventlog.kind.scan_deleted" },
  { value: "export", key: "eventlog.kind.export" },
  { value: "panic_wipe", key: "eventlog.kind.panic_wipe" },
  { value: "settings_changed", key: "eventlog.kind.settings_changed" },
  { value: "retention_purged", key: "eventlog.kind.retention_purged" },
  { value: "app_started", key: "eventlog.kind.app_started" },
];

const PAGE_SIZES = [10, 20, 50, 100] as const;
type PageSize = (typeof PAGE_SIZES)[number];

export default function ActivityLog() {
  const t = useT();
  const formatError = useIpcError();

  const [entries, setEntries] = useState<EventLogEntry[] | null>(null);
  const [count, setCount] = useState<number | null>(null);
  const [query, setQuery] = useState("");
  const [kind, setKind] = useState<EventKind | "all">("all");
  const [loadError, setLoadError] = useState<string | null>(null);
  const [exporting, setExporting] = useState(false);
  const [toast, setToast] = useState<string | null>(null);
  // PR #67: pagination — slice the loaded entries client-side. The
  // reload call already caps at 500 rows, which is plenty for the
  // 10/20/50/100 paging tiers.
  const [pageSize, setPageSize] = useState<PageSize>(10);

  const reload = useCallback(async () => {
    setLoadError(null);
    try {
      const filter = {
        kinds: kind === "all" ? undefined : [kind],
        limit: 500,
      };
      const list = query.trim().length > 0
        ? await ipc.eventlogSearch(query.trim(), 500)
        : await ipc.eventlogList(filter);
      const filtered = kind !== "all" && query.trim().length > 0
        ? list.filter((e) => e.kind === kind)
        : list;
      setEntries(filtered);
      const total = await ipc.eventlogCount();
      setCount(total);
    } catch (err) {
      setLoadError(formatError(err));
      setEntries([]);
    }
  }, [formatError, kind, query]);

  useEffect(() => {
    void reload();
  }, [reload]);

  async function onExport() {
    setExporting(true);
    setToast(null);
    try {
      const ndjson = await ipc.eventlogExport();
      if (navigator.clipboard) {
        await navigator.clipboard.writeText(ndjson);
        setToast(t("eventlog.export.toast"));
      }
    } catch (err) {
      setLoadError(formatError(err));
    } finally {
      setExporting(false);
      window.setTimeout(() => setToast(null), 4000);
    }
  }

  const totalLoaded = entries?.length ?? 0;
  const visible = entries ? entries.slice(0, pageSize) : [];

  return (
    <section
      className="max-w-5xl rounded-card bg-saw-white dark:bg-saw-grey-dark border border-saw-grey-200 dark:border-saw-grey-700 p-6"
      data-testid="settings-section-activity_log"
    >
      {/* PR #67: Export button anchored at the top-right of the
          section. Search + kind filter sit in the row below. */}
      <div className="flex items-start justify-between gap-3">
        <div>
          <h2 className="text-h3 font-semibold text-saw-grey-900 dark:text-saw-beige">
            {t("eventlog.section_title")}
          </h2>
          <p className="mt-1 text-small text-saw-grey-600 dark:text-saw-grey-400">
            {t("eventlog.section_subtitle")}
          </p>
        </div>
        <Button
          variant="secondary"
          size="sm"
          onClick={() => void onExport()}
          disabled={exporting}
          data-testid="activitylog-export"
        >
          {exporting ? t("eventlog.action.export_busy") : t("eventlog.action.export")}
        </Button>
      </div>

      <div className="mt-4 flex flex-wrap items-end gap-3">
        <label className="flex flex-col gap-1 text-small text-saw-grey-700 dark:text-saw-grey-300">
          <span>{t("eventlog.search.placeholder")}</span>
          <input
            type="search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={t("eventlog.search.placeholder")}
            className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-3 py-1.5 text-body text-saw-grey-900 dark:text-saw-beige min-w-[18rem]"
            data-testid="activitylog-search"
          />
        </label>
        <Select<EventKind | "all">
          label={t("eventlog.filter.kind_label")}
          value={kind}
          options={KIND_OPTIONS.map((o) => ({
            value: o.value,
            label: t(o.key),
          }))}
          onChange={(v) => setKind(v)}
          data-testid="activitylog-kind"
        />
      </div>

      {loadError ? (
        <p
          role="alert"
          className="mt-3 rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-red"
        >
          {loadError}
        </p>
      ) : null}
      {toast ? (
        <p
          role="status"
          className="mt-3 rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-grey-700 dark:text-saw-grey-300"
          data-testid="activitylog-toast"
        >
          {toast}
        </p>
      ) : null}

      {entries === null ? (
        <p className="mt-4 text-body text-saw-grey-600 dark:text-saw-grey-400">
          {t("common.loading")}
        </p>
      ) : entries.length === 0 ? (
        <div className="mt-4">
          <EmptyState
            title={t("eventlog.empty.title")}
            body={t("eventlog.empty.body")}
          />
        </div>
      ) : (
        <>
          <div
            role="table"
            aria-label={t("eventlog.section_title")}
            className="mt-4 rounded-card border border-saw-grey-200 dark:border-saw-grey-700 overflow-hidden"
          >
            <div
              role="row"
              className="grid grid-cols-[1.2fr_1.2fr_3fr_0.9fr_0.5fr] gap-2 border-b border-saw-grey-200 dark:border-saw-grey-700 bg-saw-grey-50 dark:bg-saw-black px-4 py-2 text-small font-medium text-saw-grey-700 dark:text-saw-grey-300"
            >
              <span role="columnheader">{t("eventlog.column.when")}</span>
              <span role="columnheader">{t("eventlog.column.kind")}</span>
              <span role="columnheader">{t("eventlog.column.summary")}</span>
              <span role="columnheader">{t("eventlog.column.account")}</span>
              <span role="columnheader">{t("eventlog.column.count")}</span>
            </div>
            {visible.map((e) => (
              <div
                role="row"
                key={e.event_id}
                data-testid={`activitylog-row-${e.event_id}`}
                className="grid grid-cols-[1.2fr_1.2fr_3fr_0.9fr_0.5fr] items-start gap-2 border-b border-saw-grey-100 dark:border-saw-grey-800 px-4 py-2 last:border-b-0 text-body text-saw-grey-900 dark:text-saw-beige"
              >
                <span role="cell" className="text-small">
                  {formatDate(e.occurred_at)}
                </span>
                <span role="cell" className="text-small text-saw-grey-700 dark:text-saw-grey-300">
                  {t(`eventlog.kind.${e.kind}`)}
                </span>
                <span role="cell" className="text-small">
                  <div>{e.summary}</div>
                  {e.detail ? (
                    <div className="text-saw-grey-600 dark:text-saw-grey-400">{e.detail}</div>
                  ) : null}
                  {e.path ? (
                    <div className="text-saw-grey-500 dark:text-saw-grey-400 font-mono text-xs break-all">
                      {e.path}
                    </div>
                  ) : null}
                </span>
                <span role="cell" className="text-small text-saw-grey-700 dark:text-saw-grey-300">
                  {e.aws_account_id_masked ?? "—"}
                </span>
                <span role="cell" className="text-small text-saw-grey-700 dark:text-saw-grey-300">
                  {e.item_count ?? "—"}
                </span>
              </div>
            ))}
          </div>

          {/* PR #67: pagination footer — counter on the left ("Showing
              N of M"), rows-per-page select on the right. The counter
              prefers the SQLite total (`count`) when available so the
              user sees the true table size, not just the filtered
              loaded subset. */}
          <div className="mt-3 flex flex-wrap items-center justify-between gap-3 text-small text-saw-grey-600 dark:text-saw-grey-400">
            <span data-testid="activitylog-counter">
              {t("eventlog.pagination.showing")
                .replace("{visible}", String(visible.length))
                .replace("{total}", String(count ?? totalLoaded))}
            </span>
            <label className="flex items-center gap-2">
              <span>{t("eventlog.pagination.rows_label")}</span>
              <select
                value={pageSize}
                onChange={(e) => setPageSize(Number(e.target.value) as PageSize)}
                data-testid="activitylog-page-size"
                className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-2 py-1 text-small text-saw-grey-900 dark:text-saw-beige focus:outline-none focus:ring-2 focus:ring-saw-red"
              >
                {PAGE_SIZES.map((size) => (
                  <option key={size} value={size}>
                    {size}
                  </option>
                ))}
              </select>
            </label>
          </div>
        </>
      )}
    </section>
  );
}

function formatDate(iso: string): string {
  try {
    return new Date(iso).toLocaleString();
  } catch {
    return iso;
  }
}
