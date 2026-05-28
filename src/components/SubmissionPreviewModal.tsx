// Shared "review before submitting" modal used by both the error
// reporting flow (Contract 12A) and the per-finding ticket flow
// (Contract 12B). Contract 12 §Constraints + §Acceptance Criteria:
// before any direct API submission we display the EXACT content
// (title + body + labels + bundle) and proceed only on explicit
// user action. The browser fallback is always available as a
// per-report choice — including when a token is configured.

import { useState } from "react";

import Button from "./Button";
import Modal from "./Modal";
import { useT } from "@/hooks/useT";
import { useIpcError } from "@/hooks/useIpcError";
import type { BrowserSubmission, IssueCreated, IssuePreview } from "@/lib/ipc";

type Props = {
  preview: IssuePreview | null;
  onClose: () => void;
  /** Direct API submit. Pass the same `preview` so what the user saw
   * is what gets submitted. Implementations either call
   * `ipc.githubSubmitErrorReport` or `ipc.githubSubmitFindingTicket`. */
  onSubmitApi: (preview: IssuePreview) => Promise<IssueCreated | { repo: { owner: string; name: string }; issue_number: number; issue_url: string }>;
  /** Browser-fallback URL builder. Always rendered as an option even
   * when a token is configured. */
  onBrowserFallback: (preview: IssuePreview) => Promise<BrowserSubmission>;
  /** True when the user has a configured PAT. Controls the primary CTA
   * label (Submit via API vs. Open in browser). */
  tokenConfigured: boolean;
};

export default function SubmissionPreviewModal({
  preview,
  onClose,
  onSubmitApi,
  onBrowserFallback,
  tokenConfigured,
}: Props) {
  const t = useT();
  const formatError = useIpcError();
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [created, setCreated] = useState<IssueCreated | null>(null);

  if (!preview) return null;

  async function submitApi() {
    if (!preview) return;
    setBusy(true);
    setErr(null);
    try {
      const r = await onSubmitApi(preview);
      setCreated(r as IssueCreated);
    } catch (e) {
      setErr(formatError(e));
    } finally {
      setBusy(false);
    }
  }

  async function openInBrowser() {
    if (!preview) return;
    try {
      const r = await onBrowserFallback(preview);
      window.open(r.url, "_blank", "noopener,noreferrer");
      // Also copy the body to the clipboard so the user can paste it
      // into the prefilled new-issue form even if the URL was truncated
      // by GitHub's title/body length limits.
      void navigator.clipboard?.writeText(preview.body).catch(() => undefined);
    } catch (e) {
      setErr(formatError(e));
    }
  }

  if (created) {
    return (
      <Modal
        open={true}
        onClose={onClose}
        title={t("submitpreview.success.title")}
        footer={
          <>
            <Button variant="ghost" onClick={onClose}>
              {t("submitpreview.success.close")}
            </Button>
            <Button
              variant="primary"
              onClick={() => {
                window.open(created.issue_url, "_blank", "noopener,noreferrer");
              }}
              data-testid="submitpreview-view-issue"
            >
              {t("submitpreview.success.view")}
            </Button>
          </>
        }
      >
        <p>
          {t("submitpreview.success.body")
            .replace("{repo}", `${created.repo.owner}/${created.repo.name}`)
            .replace("{n}", String(created.issue_number))}
        </p>
      </Modal>
    );
  }

  return (
    <Modal
      open={true}
      onClose={onClose}
      title={t("submitpreview.title")}
      footer={
        <>
          <Button variant="ghost" onClick={onClose} disabled={busy}>
            {t("submitpreview.back")}
          </Button>
          <Button
            variant="secondary"
            onClick={() => void openInBrowser()}
            disabled={busy}
            data-testid="submitpreview-browser"
          >
            {t("submitpreview.submit_browser")}
          </Button>
          <Button
            variant="primary"
            onClick={() => void submitApi()}
            disabled={busy || !tokenConfigured}
            data-testid="submitpreview-submit"
          >
            {busy ? t("submitpreview.submitting") : t("submitpreview.submit_api")}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-3 text-small text-saw-grey-800 dark:text-saw-beige">
        <p>{t("submitpreview.subtitle")}</p>
        <div>
          <div className="font-medium text-saw-grey-900 dark:text-saw-beige">
            {t("submitpreview.repo_label")}
          </div>
          <div className="font-mono text-saw-grey-700 dark:text-saw-grey-300">
            {preview.repo.owner}/{preview.repo.name}
          </div>
        </div>
        <div>
          <div className="font-medium text-saw-grey-900 dark:text-saw-beige">
            {t("submitpreview.issue_title")}
          </div>
          <div className="font-mono text-saw-grey-700 dark:text-saw-grey-300">{preview.title}</div>
        </div>
        <div>
          <div className="font-medium text-saw-grey-900 dark:text-saw-beige">
            {t("submitpreview.labels")}
          </div>
          <div className="flex flex-wrap gap-1">
            {preview.labels.map((l) => (
              <span
                key={l}
                className="rounded-full bg-saw-grey-100 dark:bg-saw-grey-800 px-2 py-0.5 text-xs text-saw-grey-700 dark:text-saw-grey-300"
              >
                {l}
              </span>
            ))}
          </div>
        </div>
        <div>
          <div className="font-medium text-saw-grey-900 dark:text-saw-beige">
            {t("submitpreview.issue_body")}
          </div>
          <pre
            className="mt-1 max-h-72 overflow-auto whitespace-pre-wrap rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-grey-50 dark:bg-saw-black p-2 font-mono text-xs text-saw-grey-800 dark:text-saw-beige"
            data-testid="submitpreview-body"
          >
            {preview.body}
          </pre>
        </div>
        {err ? (
          <p
            role="alert"
            className="rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-red"
            data-testid="submitpreview-error"
          >
            {err}
          </p>
        ) : null}
      </div>
    </Modal>
  );
}
