// PR #81 — Standalone "Create GitHub Issue" affordance for the Findings
// drawer header. Self-contained — owns its own ticket / settings fetch +
// SubmissionPreviewModal state — so it can be embedded in the drawer's
// `headerAction` slot without forcing the drawer to know about the
// underlying IPC surface.
//
// States the button renders:
//   1. Loading                — null (no spinner; the drawer body is what
//                                the user is reading)
//   2. Token + repo configured, no ticket
//      → enabled button "Create GitHub Issue" → submission flow
//   3. Token + repo configured, ticket exists
//      → "View issue #N" linking to the issue URL
//   4. Either token OR repo NOT configured
//      → disabled button with hover-reveal chip:
//        "GitHub not configured. Enable this feature in settings."
//      Matches item 2 of the 2026-05-29 user batch verbatim.

import { useCallback, useEffect, useState } from "react";

import { Button, SubmissionPreviewModal } from "@/components";
import { useT } from "@/hooks/useT";
import { useIpcError } from "@/hooks/useIpcError";
import type {
  FindingTicket,
  GithubSettings,
  IssueCreated,
  IssuePreview,
} from "@/lib/ipc";
import { ipc } from "@/lib/ipc";

type Props = {
  findingId: string;
  /** Optional callback fired after a successful issue creation so the
   * parent can refresh any state that depends on the ticket (e.g. the
   * legacy in-panel ticket row, if still rendered). */
  onCreated?: () => void;
};

export default function FindingGitHubAction({ findingId, onCreated }: Props) {
  const t = useT();
  const formatError = useIpcError();
  const [github, setGithub] = useState<GithubSettings | null>(null);
  const [ticket, setTicket] = useState<FindingTicket | null>(null);
  const [preview, setPreview] = useState<IssuePreview | null>(null);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);

  const reload = useCallback(async () => {
    try {
      const [gh, t2] = await Promise.all([
        ipc.githubGetSettings(),
        ipc.githubGetFindingTicket(findingId),
      ]);
      setGithub(gh);
      setTicket(t2);
    } catch {
      // GH config errors are non-fatal here — they collapse the button
      // into the "not configured" state which is also what we'd show
      // on a missing PAT. The parent's main IPC error path surfaces
      // anything more interesting.
    } finally {
      setLoaded(true);
    }
  }, [findingId]);

  useEffect(() => {
    void reload();
  }, [reload]);

  const configured = !!(github && github.findings_repo);

  async function onCreate() {
    if (!configured) return;
    if (ticket) return;
    setSubmitError(null);
    try {
      const p = await ipc.githubPrepareFindingTicket(
        findingId,
        github!.findings_repo!,
      );
      setPreview(p);
    } catch (err) {
      setSubmitError(formatError(err));
    }
  }

  async function onSubmitApi(p: IssuePreview): Promise<IssueCreated> {
    const created = await ipc.githubSubmitFindingTicket(findingId, p);
    // Refresh the local ticket cache so the next render shows the
    // "View issue" affordance instead of the create button.
    await reload();
    onCreated?.();
    return created;
  }

  if (!loaded) return null;

  // Ticket exists → render a small "View issue #N" link, not a button.
  if (ticket) {
    return (
      <a
        href={ticket.issue_url}
        target="_blank"
        rel="noopener noreferrer"
        data-testid="finding-drawer-ticket-link"
        className="inline-flex items-center gap-2 rounded-card border border-saw-grey-300 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-3 py-1.5 text-small font-medium text-saw-grey-800 dark:text-saw-beige hover:bg-saw-grey-50 dark:hover:bg-saw-grey-800 focus:outline-none focus-visible:ring-2 focus-visible:ring-saw-orange"
      >
        <svg viewBox="0 0 16 16" className="h-4 w-4" aria-hidden="true">
          <path
            d="M8 0C3.58 0 0 3.58 0 8a8 8 0 005.47 7.59c.4.07.55-.17.55-.38v-1.5c-2.23.48-2.7-1.07-2.7-1.07-.36-.91-.89-1.16-.89-1.16-.73-.5.06-.49.06-.49.8.06 1.23.83 1.23.83.72 1.22 1.87.87 2.33.66.07-.52.28-.87.5-1.07-1.78-.2-3.64-.89-3.64-3.96 0-.87.31-1.59.83-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.22 2.2.82a7.5 7.5 0 014 0c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.52.56.83 1.28.83 2.15 0 3.07-1.86 3.75-3.64 3.96.29.25.54.73.54 1.48v2.2c0 .21.15.46.55.38A8 8 0 0016 8c0-4.42-3.58-8-8-8z"
            fill="currentColor"
          />
        </svg>
        <span>
          {t("findingticket.drawer.view_link").replace(
            "{n}",
            String(ticket.issue_number),
          )}
        </span>
      </a>
    );
  }

  // Either token or repo missing → disabled button with hover chip.
  if (!configured) {
    return (
      <div
        className="group relative"
        data-testid="finding-drawer-gh-unconfigured"
      >
        <Button
          variant="secondary"
          size="sm"
          disabled
          aria-describedby="finding-drawer-gh-unconfigured-hint"
        >
          {t("findingticket.cta")}
        </Button>
        {/* Hover-reveal chip. `pointer-events-none` so it doesn't
            intercept the click that lands on the disabled button when
            the user double-checks the affordance is genuinely dead. */}
        <span
          id="finding-drawer-gh-unconfigured-hint"
          role="tooltip"
          className="pointer-events-none absolute right-0 top-full z-50 mt-1 max-w-xs whitespace-normal rounded bg-saw-grey-800 dark:bg-saw-grey-700 px-2 py-1 text-xs text-saw-white opacity-0 transition-opacity duration-150 group-hover:opacity-100 group-focus-within:opacity-100"
        >
          {t("findingticket.drawer.unconfigured_hint")}
        </span>
      </div>
    );
  }

  // Configured + no ticket → enabled create button.
  return (
    <>
      <Button
        variant="primary"
        size="sm"
        onClick={() => void onCreate()}
        data-testid="finding-drawer-create-issue"
      >
        {t("findingticket.cta")}
      </Button>
      {submitError ? (
        <p
          role="alert"
          className="ml-2 text-xs text-saw-red"
          data-testid="finding-drawer-gh-error"
        >
          {submitError}
        </p>
      ) : null}
      <SubmissionPreviewModal
        preview={preview}
        onClose={() => setPreview(null)}
        onSubmitApi={onSubmitApi}
        onBrowserFallback={(p) => ipc.githubBrowserFallbackForFinding(p)}
        tokenConfigured={github?.token.configured ?? false}
      />
    </>
  );
}
