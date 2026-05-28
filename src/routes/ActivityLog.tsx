// Activity log route — Contract 11A.
//
// Reachable from Settings → "Open activity log". The list is searchable
// and filterable by event kind. "Clear all" clears only the VIEW —
// underlying rows persist subject to the event-log retention policy and
// still appear in Export.

import { useCallback, useEffect, useState } from "react";

import { BackBreadcrumb, Button, EmptyState, Select } from "@/components";
import { useT } from "@/hooks/useT";
import { useIpcError } from "@/hooks/useIpcError";
import { ipc, type EventKind, type EventLogEntry } from "@/lib/ipc";

type Props = { onBack: () => void };

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

export default function ActivityLog({ onBack }: Props) {
  const t = useT();
  const formatError = useIpcError();

  const [entries, setEntries] = useState<EventLogEntry[] | null>(null);
  const [count, setCount] = useState<number | null>(null);
  const [query, setQuery] = useState("");
  const [kind, setKind] = useState<EventKind | "all">("all");
  const [loadError, setLoadError] = useState<string | null>(null);
  const [exporting, setExporting] = useState(false);
  const [toast, setToast] = useState<string | null>(null);

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

  async function onClearView() {
    try {
      await ipc.eventlogClearView();
      await reload();
    } catch (err) {
      setLoadError(formatError(err));
    }
  }

  return (
    <main className="min-h-full bg-saw-grey-50 dark:bg-saw-black px-8 py-10">
      <header className="mb-6">
        <BackBreadcrumb
          destination={t("nav.settings")}
          onBack={onBack}
          data-testid="activitylog-back"
        />
        <h1 className="mt-2 text-h1 font-semibold text-saw-grey-900 dark:text-saw-beige">
          {t("eventlog.title")}
        </h1>
        <p className="mt-1 text-small text-saw-grey-600 dark:text-saw-grey-400">
          {t("eventlog.subtitle")}
        </p>
      </header>

      <section className="max-w-5xl rounded-card bg-saw-white dark:bg-saw-grey-dark border border-saw-grey-200 dark:border-saw-grey-700 p-6">
        <div className="mb-4 flex flex-wrap items-end gap-3">
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
          <div className="ml-auto flex gap-2">
            <Button
              variant="secondary"
              size="sm"
              onClick={() => void reload()}
              data-testid="activitylog-refresh"
            >
              {t("eventlog.action.refresh")}
            </Button>
            <Button
              variant="secondary"
              size="sm"
              onClick={() => void onExport()}
              disabled={exporting}
              data-testid="activitylog-export"
            >
              {exporting ? t("eventlog.action.export_busy") : t("eventlog.action.export")}
            </Button>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => void onClearView()}
              data-testid="activitylog-clear-view"
            >
              {t("eventlog.action.clear_view")}
            </Button>
          </div>
        </div>

        {loadError ? (
          <p
            role="alert"
            className="mb-3 rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-red"
          >
            {loadError}
          </p>
        ) : null}
        {toast ? (
          <p
            role="status"
            className="mb-3 rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-grey-700 dark:text-saw-grey-300"
            data-testid="activitylog-toast"
          >
            {toast}
          </p>
        ) : null}

        {entries === null ? (
          <p className="text-body text-saw-grey-600 dark:text-saw-grey-400">{t("common.loading")}</p>
        ) : entries.length === 0 ? (
          <EmptyState
            title={t("eventlog.empty.title")}
            body={t("eventlog.empty.body")}
          />
        ) : (
          <>
            <p className="mb-2 text-small text-saw-grey-600 dark:text-saw-grey-400">
              {t("eventlog.count_total").replace(
                "{count}",
                String(count ?? entries.length),
              )}
            </p>
            <div
              role="table"
              aria-label={t("eventlog.title")}
              className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 overflow-hidden"
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
              {entries.map((e) => (
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
          </>
        )}
      </section>
    </main>
  );
}

function formatDate(iso: string): string {
  try {
    return new Date(iso).toLocaleString();
  } catch {
    return iso;
  }
}
