// Bug-report affordance — small grey flag at the bottom-left of
// every authenticated screen (left of the VersionFooter).
//
// PR #68: replaces the dedicated "Error-report destination" +
// "Report a security bug" rows in Settings → GitHub. The user
// wanted bug-reporting to be a quiet, always-available surface
// instead of buried inside Settings.
//
// Behavior:
//   - Idle: a small grey flag glyph (⚑). A hover label appears
//     beside it that reads "Report a bug" so the affordance is
//     discoverable without a tooltip delay.
//   - Click: opens a modal with two routes:
//       * "Report on GitHub" → the public CloudSaw issues page,
//         opened in the OS default browser via the Tauri opener
//         plugin (PR #68).
//       * "Email security@cloud-saw.com" → opens the OS default
//         mail client to the same security contact previously
//         shown statically in Settings → GitHub.
//   - The opener plugin handles the OS hand-off; if it rejects
//     the URL the modal surfaces a short error string.
//
// Styling note: the flag uses pointer-events-auto on the host so
// it remains clickable even though the VersionFooter container
// next to it is pointer-events-none.

import { useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";

import { Button, Modal } from "@/components";
import { useT } from "@/hooks/useT";

const GITHUB_ISSUES_URL = "https://github.com/Camdun-Solutions/CloudSaw/issues/new";
const SECURITY_MAILTO = "mailto:security@cloud-saw.com";

export default function ReportBugFlag() {
  const t = useT();
  const [open, setOpen] = useState(false);
  const [openError, setOpenError] = useState<string | null>(null);

  async function openExternal(url: string) {
    setOpenError(null);
    try {
      await openUrl(url);
    } catch (err) {
      // Keep the modal open so the user sees what failed and can
      // copy the URL manually. The plugin error shape is opaque —
      // formatError lives in a different hook and would force a
      // larger import dance here; the short raw string is enough.
      const msg =
        typeof err === "string"
          ? err
          : err instanceof Error
            ? err.message
            : "unknown";
      setOpenError(msg);
    }
  }

  return (
    <>
      {/* PR #77 — z-50 (was z-40) so the hover label paints above
          modals' backdrop chrome and above the floating TopNav.
          The label itself was being clipped by the bottom-left
          window edge — the flag sits at `left-2` and the chip used
          `left-1/2 -translate-x-1/2` (centered above the flag) so
          half the chip sat outside the viewport. Anchoring the
          chip to the LEFT edge of the flag (left-0, no centering)
          keeps it on-screen on every viewport width. */}
      <div
        className="group fixed bottom-2 left-2 z-50"
        data-testid="report-bug-flag"
      >
        <button
          type="button"
          onClick={() => {
            setOpenError(null);
            setOpen(true);
          }}
          aria-label={t("report_bug.flag_label")}
          // PR #69: `title` removed — the custom hover chip below
          // already surfaces the label, and the native tooltip
          // created a second copy showing on top of it.
          className="inline-flex h-6 w-6 items-center justify-center rounded text-saw-grey-500 hover:text-saw-grey-700 dark:text-saw-grey-400 dark:hover:text-saw-grey-200 focus:outline-none focus-visible:ring-2 focus-visible:ring-saw-red"
          data-testid="report-bug-flag-button"
        >
          <span aria-hidden="true" className="text-base leading-none">⚑</span>
        </button>
        {/* Hover-reveal label — pops ABOVE the flag as a small chip
            so it doesn't push the adjacent VersionFooter sideways.
            Appears on hover and on keyboard focus. */}
        <span
          className="pointer-events-none absolute bottom-full left-0 z-50 mb-1 whitespace-nowrap rounded bg-saw-grey-800 dark:bg-saw-grey-700 px-2 py-0.5 text-xs text-saw-white opacity-0 transition-opacity duration-150 group-hover:opacity-100 group-focus-within:opacity-100"
          data-testid="report-bug-flag-label"
        >
          {t("report_bug.flag_label")}
        </span>
      </div>

      <Modal
        open={open}
        onClose={() => setOpen(false)}
        title={t("report_bug.modal_title")}
        footer={
          <Button
            variant="ghost"
            onClick={() => setOpen(false)}
            data-testid="report-bug-close"
          >
            {t("common.close")}
          </Button>
        }
      >
        <div className="flex flex-col gap-3">
          <p className="text-body text-saw-grey-700 dark:text-saw-grey-300">
            {t("report_bug.modal_body")}
          </p>
          {/* PR #69: center the two CTAs. The previous layout pinned
              the buttons against the left edge; the user wanted the
              row centered within the modal body. */}
          <div className="flex flex-col gap-2 sm:flex-row sm:justify-center">
            <Button
              variant="primary"
              onClick={() => void openExternal(GITHUB_ISSUES_URL)}
              data-testid="report-bug-github"
            >
              {t("report_bug.github_cta")}
            </Button>
            <Button
              variant="secondary"
              onClick={() => void openExternal(SECURITY_MAILTO)}
              data-testid="report-bug-email"
            >
              {t("report_bug.email_cta")}
            </Button>
          </div>
          <p className="text-xs text-saw-grey-500 dark:text-saw-grey-400">
            {t("report_bug.email_hint")}
          </p>
          {openError ? (
            <p
              role="alert"
              className="rounded-card bg-saw-grey-100 dark:bg-saw-grey-800 px-3 py-2 text-small text-saw-red"
              data-testid="report-bug-error"
            >
              {t("report_bug.open_failed").replace("{detail}", openError)}
            </p>
          ) : null}
        </div>
      </Modal>
    </>
  );
}
