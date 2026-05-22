// Export-report modal — Contract 15.
//
// Drives the per-scan export flow. The user picks a format (HTML or
// PDF) and a disclosure mode (masked or full account IDs), opens the
// native save dialog to choose an output path, and submits. The Rust
// side validates the path shape, writes the file atomically, and
// returns the outcome.
//
// The output path NEVER comes from the user typing it free-form — the
// "Choose location…" button calls the Tauri dialog plugin, which
// returns either a string path or null (user canceled). The text
// field below is a read-only display so the user can verify what was
// picked before submitting.

import { useEffect, useState } from "react";

import { save } from "@tauri-apps/plugin-dialog";

import Button from "./Button";
import Modal from "./Modal";
import { useT } from "@/hooks/useT";
import { useIpcError } from "@/hooks/useIpcError";
import {
  ipc,
  type AccountIdDisclosure,
  type ExportOutcome,
} from "@/lib/ipc";

type Format = "html" | "pdf";

type Props = {
  scanId: string | null;
  onClose: () => void;
};

export default function ExportReportDialog({ scanId, onClose }: Props) {
  const t = useT();
  const formatError = useIpcError();
  const [format, setFormat] = useState<Format>("html");
  const [showFullIds, setShowFullIds] = useState(false);
  const [path, setPath] = useState("");
  const [pickerBusy, setPickerBusy] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [outcome, setOutcome] = useState<ExportOutcome | null>(null);

  // Hydrate the disclosure default from settings whenever the modal
  // opens. Closing wipes local state.
  useEffect(() => {
    if (!scanId) return;
    setFormat("html");
    setShowFullIds(false);
    setPath("");
    setError(null);
    setOutcome(null);
    void ipc.reportGetSettings().then(
      (s) => setShowFullIds(!s.mask_account_ids_default),
      () => undefined,
    );
  }, [scanId]);

  if (!scanId) return null;

  const disclosure: AccountIdDisclosure = showFullIds ? "full" : "masked";

  async function choosePath() {
    setPickerBusy(true);
    setError(null);
    try {
      const ext = format === "pdf" ? "pdf" : "html";
      const defaultPath = `cloudsaw-scan-${scanId!.slice(0, 8)}.${ext}`;
      const picked = await save({
        defaultPath,
        filters: [
          format === "pdf"
            ? { name: "PDF", extensions: ["pdf"] }
            : { name: "HTML", extensions: ["html", "htm"] },
        ],
      });
      // `save` returns null when the user cancels — Contract 15
      // §Edge Cases: "Save dialog canceled → no file is written and
      // no error is shown." We just leave the existing path
      // untouched.
      if (picked && typeof picked === "string") {
        setPath(picked);
      }
    } catch (e) {
      setError(formatError(e));
    } finally {
      setPickerBusy(false);
    }
  }

  async function submit() {
    if (!path || !scanId) return;
    setSubmitting(true);
    setError(null);
    try {
      const fn =
        format === "pdf"
          ? ipc.reportExportScanPdf
          : ipc.reportExportScanHtml;
      const result = await fn(scanId, path, disclosure);
      setOutcome(result);
    } catch (e) {
      setError(formatError(e));
    } finally {
      setSubmitting(false);
    }
  }

  if (outcome) {
    return (
      <Modal
        open={true}
        onClose={onClose}
        title={t("report.export.success_title")}
        footer={
          <Button variant="primary" onClick={onClose} data-testid="export-success-close">
            {t("common.close")}
          </Button>
        }
      >
        <div className="flex flex-col gap-2 text-small text-saw-grey-800">
          <p>
            {t("report.export.success_body")
              .replace("{bytes}", outcome.bytes_written.toLocaleString())
              .replace("{path}", outcome.primary_path)}
          </p>
          {outcome.auto_export_path ? (
            <p className="text-saw-grey-700">
              {t("report.export.auto_ok").replace("{path}", outcome.auto_export_path)}
            </p>
          ) : outcome.auto_export_failed ? (
            <p
              className="rounded-card bg-saw-grey-100 px-3 py-2 text-saw-red"
              data-testid="export-auto-failed"
            >
              {t("report.export.auto_failed")}
            </p>
          ) : null}
        </div>
      </Modal>
    );
  }

  return (
    <Modal
      open={true}
      onClose={onClose}
      title={t("report.export.title")}
      footer={
        <>
          <Button variant="ghost" onClick={onClose} disabled={submitting}>
            {t("report.export.cancel")}
          </Button>
          <Button
            variant="primary"
            onClick={() => void submit()}
            disabled={submitting || !path}
            data-testid="export-submit"
          >
            {submitting ? t("report.export.submitting") : t("report.export.submit")}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-3 text-small text-saw-grey-800">
        <p>{t("report.export.body")}</p>

        <fieldset>
          <legend className="text-saw-grey-900 font-medium">
            {t("report.export.format_label")}
          </legend>
          <label className="mr-4">
            <input
              type="radio"
              name="export-format"
              checked={format === "html"}
              onChange={() => setFormat("html")}
              data-testid="export-format-html"
            />{" "}
            {t("report.export.format.html")}
          </label>
          <label>
            <input
              type="radio"
              name="export-format"
              checked={format === "pdf"}
              onChange={() => setFormat("pdf")}
              data-testid="export-format-pdf"
            />{" "}
            {t("report.export.format.pdf")}
          </label>
        </fieldset>

        <label className="flex items-start gap-2">
          <input
            type="checkbox"
            checked={showFullIds}
            onChange={(e) => setShowFullIds(e.target.checked)}
            className="mt-1"
            data-testid="export-disclosure"
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
            variant="secondary"
            onClick={() => void choosePath()}
            disabled={pickerBusy || submitting}
            data-testid="export-choose-path"
          >
            {pickerBusy
              ? t("report.export.choose_path_busy")
              : t("report.export.choose_path")}
          </Button>
        </div>

        <label className="flex flex-col gap-1">
          <span className="text-saw-grey-900 font-medium">
            {t("report.export.path_label")}
          </span>
          <input
            type="text"
            readOnly
            value={path}
            placeholder={t("report.export.path_placeholder")}
            className="rounded-card border border-saw-grey-200 bg-saw-grey-50 px-3 py-1.5 text-body text-saw-grey-900 font-mono"
            data-testid="export-path"
          />
        </label>

        {error ? (
          <p
            role="alert"
            className="rounded-card bg-saw-grey-100 px-3 py-2 text-saw-red"
            data-testid="export-error"
          >
            {error}
          </p>
        ) : null}
      </div>
    </Modal>
  );
}
