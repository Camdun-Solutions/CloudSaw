// Error reporting dialog — Contract 12A.
//
// Shown when CloudSaw hits an unhandled error (or when the user opens
// the "Report a problem" affordance manually). Surfaces:
//
//   * Save diagnostic bundle (copies the redacted bundle to clipboard).
//   * File bug report (opens the SubmissionPreviewModal — the user
//     reviews the EXACT content before any direct API call).
//   * Configure GitHub token (when no token is configured).
//   * security@cloud-saw.com (the channel for sensitive reports).
//
// The browser fallback is always available — the SubmissionPreviewModal
// renders both "Submit via API" and "Open in browser" buttons.

import { useCallback, useEffect, useState } from "react";

import Button from "./Button";
import Modal from "./Modal";
import SubmissionPreviewModal from "./SubmissionPreviewModal";
import { useT } from "@/hooks/useT";
import { useIpcError } from "@/hooks/useIpcError";
import { useLocale } from "@/stores/locale";
import { ipc, type GithubSettings, type IssuePreview } from "@/lib/ipc";

type Props = {
  open: boolean;
  /** Pre-fills the notes field. Surfaced by an error boundary so the
   * "what were you doing" copy starts with the failing message. */
  initialNotes?: string;
  onClose: () => void;
  /** Opens the Settings → GitHub section so the user can paste a PAT. */
  onConfigureToken: () => void;
};

export default function ErrorReportDialog({
  open,
  initialNotes,
  onClose,
  onConfigureToken,
}: Props) {
  const t = useT();
  const { locale } = useLocale();
  const formatError = useIpcError();
  const [notes, setNotes] = useState(initialNotes ?? "");
  const [settings, setSettings] = useState<GithubSettings | null>(null);
  const [bundleSaved, setBundleSaved] = useState(false);
  const [contactCopied, setContactCopied] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [preview, setPreview] = useState<IssuePreview | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setSettings(await ipc.githubGetSettings());
    } catch (e) {
      setErr(formatError(e));
    }
  }, [formatError]);

  useEffect(() => {
    if (open) {
      setNotes(initialNotes ?? "");
      setBundleSaved(false);
      setContactCopied(false);
      setErr(null);
      setPreview(null);
      void refresh();
    }
  }, [open, initialNotes, refresh]);

  async function saveBundle() {
    setErr(null);
    setBundleSaved(false);
    try {
      const p = await ipc.githubPrepareErrorReport(notes || null, locale);
      // The "save" action copies the rendered body to the clipboard.
      // Pasting into a file or pastebin is the user's choice — we never
      // open a file dialog here, so the bundle never leaves the user's
      // explicit control.
      if (navigator.clipboard) {
        await navigator.clipboard.writeText(p.body);
      }
      setBundleSaved(true);
      window.setTimeout(() => setBundleSaved(false), 4000);
    } catch (e) {
      setErr(formatError(e));
    }
  }

  async function fileBugReport() {
    setErr(null);
    setBusy(true);
    try {
      const p = await ipc.githubPrepareErrorReport(notes || null, locale);
      setPreview(p);
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setBusy(false);
    }
  }

  function copyContact() {
    if (!settings) return;
    if (navigator.clipboard) {
      void navigator.clipboard.writeText(settings.security_contact);
      setContactCopied(true);
      window.setTimeout(() => setContactCopied(false), 2000);
    }
  }

  if (preview && settings) {
    return (
      <SubmissionPreviewModal
        preview={preview}
        onClose={() => setPreview(null)}
        onSubmitApi={(p) => ipc.githubSubmitErrorReport(p)}
        onBrowserFallback={(p) => ipc.githubBrowserFallbackForError(p)}
        tokenConfigured={settings.token.configured}
      />
    );
  }

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={t("errordialog.title")}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {t("errordialog.dismiss")}
          </Button>
          <Button
            variant="secondary"
            onClick={() => void saveBundle()}
            data-testid="errordialog-save-bundle"
          >
            {bundleSaved
              ? t("errordialog.save_bundle_done")
              : t("errordialog.save_bundle")}
          </Button>
          {settings && !settings.token.configured ? (
            <Button
              variant="secondary"
              onClick={onConfigureToken}
              data-testid="errordialog-configure-token"
            >
              {t("errordialog.configure_token")}
            </Button>
          ) : null}
          <Button
            variant="primary"
            onClick={() => void fileBugReport()}
            disabled={busy}
            data-testid="errordialog-file-bug"
          >
            {t("errordialog.file_bug")}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-3 text-small text-saw-grey-800 dark:text-saw-beige">
        <p>{t("errordialog.subtitle")}</p>

        <label className="flex flex-col gap-1">
          <span>{t("errordialog.notes_label")}</span>
          <textarea
            value={notes}
            onChange={(e) => setNotes(e.target.value)}
            placeholder={t("errordialog.notes_placeholder")}
            rows={4}
            className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-3 py-1.5 text-body text-saw-grey-900 dark:text-saw-beige"
            data-testid="errordialog-notes"
          />
        </label>

        {settings ? (
          <div
            className="rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-grey-50 dark:bg-saw-black px-3 py-2 text-small text-saw-grey-700 dark:text-saw-grey-300"
            data-testid="errordialog-security-note"
          >
            <p>
              {t("errordialog.security_note").replace("{email}", settings.security_contact)}
            </p>
            <div className="mt-1 flex items-center gap-2">
              <span className="font-mono text-saw-grey-900 dark:text-saw-beige">
                {settings.security_contact}
              </span>
              <button
                type="button"
                onClick={copyContact}
                className="text-xs underline underline-offset-2"
                data-testid="errordialog-copy-email"
              >
                {contactCopied ? t("errordialog.copied") : t("errordialog.copy_email")}
              </button>
            </div>
          </div>
        ) : null}

        {err ? (
          <p
            role="alert"
            className="rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-red"
          >
            {err}
          </p>
        ) : null}
      </div>
    </Modal>
  );
}
