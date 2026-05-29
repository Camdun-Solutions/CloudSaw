// PR #70 — Activity-log export modal.
//
// Replaces the old "click Export → copy NDJSON to clipboard"
// affordance. The user now picks:
//   * a file format (HTML, PDF, or Excel)
//   * an optional date range (start + end)
//   * an optional activity type filter
//
// On submit, the modal opens the OS save dialog at a sensible
// default path, then calls the matching Rust IPC. The IPC writes
// the themed file to disk and returns an outcome the modal
// surfaces in a success row.

import { useEffect, useMemo, useState } from "react";

import { save } from "@tauri-apps/plugin-dialog";

import { Button, Modal } from "@/components";
import { useT } from "@/hooks/useT";
import { useIpcError } from "@/hooks/useIpcError";
import {
  ipc,
  type EventKind,
  type EventLogExportOutcome,
  type EventLogFilter,
} from "@/lib/ipc";

type Format = "html" | "pdf" | "xlsx";

// PR #71: multi-select activity-type checkbox grid. The pseudo
// "all" sentinel from the old single-select dropdown is gone —
// an empty `selectedKinds` set is the new "all" signal.
const KIND_OPTIONS: ReadonlyArray<{ value: EventKind; key: string }> = [
  { value: "scan_completed", key: "eventlog.kind.scan_completed" },
  { value: "scan_failed", key: "eventlog.kind.scan_failed" },
  { value: "scan_canceled", key: "eventlog.kind.scan_canceled" },
  { value: "scheduled_scan_fired", key: "eventlog.kind.scheduled_scan_fired" },
  { value: "scheduled_scan_skipped", key: "eventlog.kind.scheduled_scan_skipped" },
  { value: "findings_auto_resolved", key: "eventlog.kind.findings_auto_resolved" },
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

type Props = {
  open: boolean;
  onClose: () => void;
};

export default function ExportActivityLogModal({ open, onClose }: Props) {
  const t = useT();
  const formatError = useIpcError();
  const today = useMemo(() => new Date().toISOString().slice(0, 10), []);
  const ninetyDaysAgo = useMemo(
    () =>
      new Date(Date.now() - 90 * 24 * 60 * 60 * 1000)
        .toISOString()
        .slice(0, 10),
    [],
  );

  const [format, setFormat] = useState<Format>("html");
  const [start, setStart] = useState("");
  const [end, setEnd] = useState("");
  // PR #71: Activity Type is now a multi-select. Empty set === "all
  // kinds." A modal-local state ensures the user can incrementally
  // toggle kinds with checkboxes without juggling a sentinel value.
  const [selectedKinds, setSelectedKinds] = useState<Set<EventKind>>(new Set());
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [outcome, setOutcome] = useState<EventLogExportOutcome | null>(null);

  // Reset state whenever the modal opens.
  useEffect(() => {
    if (!open) return;
    setFormat("html");
    setStart("");
    setEnd("");
    setSelectedKinds(new Set());
    setError(null);
    setOutcome(null);
    setSubmitting(false);
  }, [open]);

  function toggleKind(k: EventKind) {
    setSelectedKinds((prev) => {
      const next = new Set(prev);
      if (next.has(k)) next.delete(k);
      else next.add(k);
      return next;
    });
  }

  function close() {
    if (submitting) return;
    onClose();
  }

  async function runExport() {
    setError(null);
    setOutcome(null);
    const ext = format === "pdf" ? "pdf" : format === "xlsx" ? "xlsx" : "html";
    const stamp = new Date().toISOString().slice(0, 10);
    const defaultPath = `cloudsaw-activity-${stamp}.${ext}`;
    let picked: string | null = null;
    try {
      const result = await save({
        defaultPath,
        filters: [
          format === "pdf"
            ? { name: "PDF", extensions: ["pdf"] }
            : format === "xlsx"
              ? { name: "Excel", extensions: ["xlsx"] }
              : { name: "HTML", extensions: ["html", "htm"] },
        ],
      });
      if (result && typeof result === "string") picked = result;
    } catch (e) {
      setError(formatError(e));
      return;
    }
    if (!picked) return; // user canceled

    const filter: EventLogFilter = {
      kinds: selectedKinds.size === 0 ? undefined : Array.from(selectedKinds),
      since: start.length > 0 ? `${start}T00:00:00Z` : null,
      until: end.length > 0 ? `${end}T23:59:59Z` : null,
      include_cleared: true,
    };

    setSubmitting(true);
    try {
      const fn =
        format === "pdf"
          ? ipc.eventlogExportPdf
          : format === "xlsx"
            ? ipc.eventlogExportXlsx
            : ipc.eventlogExportHtml;
      const r = await fn(filter, picked);
      setOutcome(r);
      // PR #71: keep the success outcome visible briefly so the user
      // sees the row-count + path before the modal dismisses. This
      // matches the close-on-success behavior the Custom Report
      // modal gained in the same PR.
      window.setTimeout(() => {
        onClose();
      }, 700);
    } catch (e) {
      setError(formatError(e));
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <Modal
      open={open}
      onClose={close}
      title={t("eventlog.export.modal_title")}
      size="lg"
      footer={
        <>
          <Button
            variant="ghost"
            onClick={close}
            disabled={submitting}
            data-testid="export-activitylog-cancel"
          >
            {t("common.cancel")}
          </Button>
          <Button
            variant="primary"
            onClick={() => void runExport()}
            disabled={submitting}
            data-testid="export-activitylog-go"
          >
            {submitting
              ? t("eventlog.export.submitting")
              : t("eventlog.export.go")}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-4">
        <p className="text-small text-saw-grey-600 dark:text-saw-grey-400">
          {t("eventlog.export.modal_body")}
        </p>

        <fieldset>
          <legend className="text-small font-medium text-saw-grey-900 dark:text-saw-beige">
            {t("eventlog.export.format_label")}
          </legend>
          <div className="mt-2 flex flex-wrap gap-4 text-small text-saw-grey-700 dark:text-saw-grey-300">
            <label className="flex items-center gap-2">
              <input
                type="radio"
                name="export-activitylog-format"
                checked={format === "html"}
                onChange={() => setFormat("html")}
                data-testid="export-activitylog-format-html"
              />
              <span>{t("eventlog.export.format_html")}</span>
            </label>
            <label className="flex items-center gap-2">
              <input
                type="radio"
                name="export-activitylog-format"
                checked={format === "pdf"}
                onChange={() => setFormat("pdf")}
                data-testid="export-activitylog-format-pdf"
              />
              <span>{t("eventlog.export.format_pdf")}</span>
            </label>
            <label className="flex items-center gap-2">
              <input
                type="radio"
                name="export-activitylog-format"
                checked={format === "xlsx"}
                onChange={() => setFormat("xlsx")}
                data-testid="export-activitylog-format-xlsx"
              />
              <span>{t("eventlog.export.format_xlsx")}</span>
            </label>
          </div>
        </fieldset>

        <div className="grid grid-cols-2 gap-3">
          <label className="flex flex-col gap-1 text-small text-saw-grey-700 dark:text-saw-grey-300">
            <span>
              {t("eventlog.export.start_label")}{" "}
              <span className="text-xs text-saw-grey-500 dark:text-saw-grey-400">
                ({t("eventlog.export.optional")})
              </span>
            </span>
            <input
              type="date"
              value={start}
              onChange={(e) => setStart(e.target.value)}
              max={end || today}
              placeholder={ninetyDaysAgo}
              className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-3 py-1.5 text-body text-saw-grey-900 dark:text-saw-beige"
              data-testid="export-activitylog-start"
            />
          </label>
          <label className="flex flex-col gap-1 text-small text-saw-grey-700 dark:text-saw-grey-300">
            <span>
              {t("eventlog.export.end_label")}{" "}
              <span className="text-xs text-saw-grey-500 dark:text-saw-grey-400">
                ({t("eventlog.export.optional")})
              </span>
            </span>
            <input
              type="date"
              value={end}
              onChange={(e) => setEnd(e.target.value)}
              min={start}
              max={today}
              className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-3 py-1.5 text-body text-saw-grey-900 dark:text-saw-beige"
              data-testid="export-activitylog-end"
            />
          </label>
        </div>

        {/* PR #71: activity-type filter is now a checkbox grid so the
            user can pick MULTIPLE kinds instead of being forced to
            "one kind OR all". Leaving every box unchecked exports
            every kind. */}
        <fieldset data-testid="export-activitylog-kind">
          <legend className="text-small font-medium text-saw-grey-900 dark:text-saw-beige">
            {t("eventlog.export.kind_label")}
          </legend>
          <p className="mt-1 text-xs text-saw-grey-500 dark:text-saw-grey-400">
            {t("eventlog.export.kind_hint")}
          </p>
          <div className="mt-2 grid grid-cols-1 gap-x-3 gap-y-1.5 sm:grid-cols-2">
            {KIND_OPTIONS.map((o) => (
              <label
                key={o.value}
                className="flex items-center gap-2 rounded px-1.5 py-0.5 text-small text-saw-grey-700 hover:bg-saw-grey-100 dark:text-saw-grey-300 dark:hover:bg-saw-grey-800"
              >
                <input
                  type="checkbox"
                  checked={selectedKinds.has(o.value)}
                  onChange={() => toggleKind(o.value)}
                  data-testid={`export-activitylog-kind-${o.value}`}
                  className="h-4 w-4 rounded border-saw-grey-300 bg-saw-white text-saw-red focus:ring-saw-red dark:border-saw-grey-600 dark:bg-saw-grey-800 dark:checked:bg-saw-red dark:focus:ring-saw-red"
                />
                <span>{t(o.key)}</span>
              </label>
            ))}
          </div>
        </fieldset>

        {error ? (
          <p
            role="alert"
            className="rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-red"
            data-testid="export-activitylog-error"
          >
            {error}
          </p>
        ) : null}
        {outcome ? (
          <div
            role="status"
            className="rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-grey-700 dark:text-saw-grey-300"
            data-testid="export-activitylog-outcome"
          >
            <p>
              {t("eventlog.export.success")
                .replace("{rows}", String(outcome.rows_exported))
                .replace("{format}", outcome.format.toUpperCase())
                .replace("{path}", outcome.primary_path)}
            </p>
          </div>
        ) : null}
      </div>
    </Modal>
  );
}
