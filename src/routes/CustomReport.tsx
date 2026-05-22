// Custom date-range report builder — Contract 15B.
//
// The user picks a start/end date, an explicit account scope (one
// account ID per line, blank = "all locally-known accounts"), the
// disclosure mode, and the format. Submitting opens the native save
// dialog for the output path and calls the corresponding IPC.
//
// The wizard does NOT generate the report on the frontend — that
// happens entirely in the Rust `reports::aggregator` + renderer. The
// frontend only assembles the request.

import { useState } from "react";

import { save } from "@tauri-apps/plugin-dialog";

import { Button } from "@/components";
import { useT } from "@/hooks/useT";
import { useIpcError } from "@/hooks/useIpcError";
import { ipc, type AccountIdDisclosure, type ExportOutcome } from "@/lib/ipc";

type Format = "html" | "pdf";

type Props = { onBack: () => void };

export default function CustomReport({ onBack }: Props) {
  const t = useT();
  const formatError = useIpcError();
  const today = new Date().toISOString().slice(0, 10);
  const ninetyDaysAgo = new Date(Date.now() - 90 * 24 * 60 * 60 * 1000)
    .toISOString()
    .slice(0, 10);
  const [start, setStart] = useState(ninetyDaysAgo);
  const [end, setEnd] = useState(today);
  const [scopeRaw, setScopeRaw] = useState("");
  const [format, setFormat] = useState<Format>("html");
  const [showFullIds, setShowFullIds] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [outcome, setOutcome] = useState<ExportOutcome | null>(null);

  async function buildAndExport() {
    setError(null);
    setOutcome(null);
    const accountScope = scopeRaw
      .split(/[\n,]/)
      .map((s) => s.trim())
      .filter((s) => s.length > 0);
    const disclosure: AccountIdDisclosure = showFullIds ? "full" : "masked";

    let picked: string | null = null;
    try {
      const ext = format === "pdf" ? "pdf" : "html";
      const defaultPath = `cloudsaw-custom-${start}-to-${end}.${ext}`;
      const result = await save({
        defaultPath,
        filters: [
          format === "pdf"
            ? { name: "PDF", extensions: ["pdf"] }
            : { name: "HTML", extensions: ["html", "htm"] },
        ],
      });
      if (result && typeof result === "string") picked = result;
    } catch (e) {
      setError(formatError(e));
      return;
    }
    if (!picked) return; // user canceled

    setSubmitting(true);
    try {
      const startIso = `${start}T00:00:00Z`;
      const endIso = `${end}T23:59:59Z`;
      const fn =
        format === "pdf"
          ? ipc.reportExportCustomPdf
          : ipc.reportExportCustomHtml;
      const r = await fn(startIso, endIso, accountScope, picked, disclosure);
      setOutcome(r);
    } catch (e) {
      setError(formatError(e));
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <main className="min-h-full bg-saw-grey-50 px-8 py-10">
      <header className="mb-6 flex items-center justify-between">
        <div>
          <h1 className="text-h1 font-semibold text-saw-grey-900">
            {t("report.custom.title")}
          </h1>
          <p className="mt-1 text-small text-saw-grey-600">
            {t("report.custom.body")}
          </p>
        </div>
        <Button variant="ghost" onClick={onBack} data-testid="custom-report-back">
          {t("common.back")}
        </Button>
      </header>

      <section className="max-w-2xl rounded-card bg-saw-white border border-saw-grey-200 p-6">
        <div className="flex flex-col gap-4">
          <div className="grid grid-cols-2 gap-3">
            <label className="flex flex-col gap-1 text-small text-saw-grey-700">
              <span>{t("report.custom.start")}</span>
              <input
                type="date"
                value={start}
                onChange={(e) => setStart(e.target.value)}
                className="rounded-card border border-saw-grey-200 bg-saw-white px-3 py-1.5 text-body text-saw-grey-900"
                data-testid="custom-report-start"
              />
            </label>
            <label className="flex flex-col gap-1 text-small text-saw-grey-700">
              <span>{t("report.custom.end")}</span>
              <input
                type="date"
                value={end}
                onChange={(e) => setEnd(e.target.value)}
                className="rounded-card border border-saw-grey-200 bg-saw-white px-3 py-1.5 text-body text-saw-grey-900"
                data-testid="custom-report-end"
              />
            </label>
          </div>

          <label className="flex flex-col gap-1 text-small text-saw-grey-700">
            <span>{t("report.custom.account_scope")}</span>
            <textarea
              value={scopeRaw}
              onChange={(e) => setScopeRaw(e.target.value)}
              placeholder={t("report.custom.account_scope_placeholder")}
              rows={3}
              className="rounded-card border border-saw-grey-200 bg-saw-white px-3 py-1.5 text-body text-saw-grey-900 font-mono"
              data-testid="custom-report-scope"
            />
            <span className="text-xs text-saw-grey-500">
              {t("report.custom.empty_scope_hint")}
            </span>
          </label>

          <fieldset>
            <legend className="text-saw-grey-900 font-medium text-small">
              {t("report.export.format_label")}
            </legend>
            <label className="mr-4 text-small text-saw-grey-700">
              <input
                type="radio"
                name="custom-report-format"
                checked={format === "html"}
                onChange={() => setFormat("html")}
                data-testid="custom-report-format-html"
              />{" "}
              {t("report.export.format.html")}
            </label>
            <label className="text-small text-saw-grey-700">
              <input
                type="radio"
                name="custom-report-format"
                checked={format === "pdf"}
                onChange={() => setFormat("pdf")}
                data-testid="custom-report-format-pdf"
              />{" "}
              {t("report.export.format.pdf")}
            </label>
          </fieldset>

          <label className="flex items-start gap-2 text-small text-saw-grey-700">
            <input
              type="checkbox"
              checked={showFullIds}
              onChange={(e) => setShowFullIds(e.target.checked)}
              className="mt-1"
              data-testid="custom-report-disclosure"
            />
            <span>
              <span className="font-medium text-saw-grey-900">
                {t("report.export.disclosure_label")}
              </span>
              <br />
              <span className="text-xs text-saw-grey-600">
                {t("report.export.disclosure_hint")}
              </span>
            </span>
          </label>

          <div>
            <Button
              variant="primary"
              onClick={() => void buildAndExport()}
              disabled={submitting || !start || !end}
              data-testid="custom-report-go"
            >
              {submitting
                ? t("report.export.submitting")
                : t("report.custom.go")}
            </Button>
          </div>

          {error ? (
            <p
              role="alert"
              className="rounded-card bg-saw-grey-100 px-3 py-2 text-small text-saw-red"
              data-testid="custom-report-error"
            >
              {error}
            </p>
          ) : null}
          {outcome ? (
            <div
              className="rounded-card bg-saw-grey-100 px-3 py-2 text-small text-saw-grey-700"
              data-testid="custom-report-outcome"
            >
              <p>
                {t("report.export.success_body")
                  .replace("{bytes}", outcome.bytes_written.toLocaleString())
                  .replace("{path}", outcome.primary_path)}
              </p>
              {outcome.auto_export_path ? (
                <p className="mt-1">
                  {t("report.export.auto_ok").replace("{path}", outcome.auto_export_path)}
                </p>
              ) : outcome.auto_export_failed ? (
                <p className="mt-1 text-saw-red">{t("report.export.auto_failed")}</p>
              ) : null}
            </div>
          ) : null}
        </div>
      </section>
    </main>
  );
}
