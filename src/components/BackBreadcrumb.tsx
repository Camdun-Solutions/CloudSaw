// Back-arrow + breadcrumb — replaces the legacy "Close" button on
// every full-page child route (Settings, Profiles, ScheduledScans,
// ActivityLog, CustomReport). PR #49 introduces it per user
// request: instead of a "Close" button, sub-pages get a left-
// arrow + "Back to {destination}" affordance so the user always
// knows where they're returning to.
//
// Visual: ghost-style link, `text-saw-grey-700` default →
// `text-saw-red` on hover. Sits in the top-left of each page's
// header (the natural reading-start position for "back" UX).
//
// Top-level destinations (Dashboard, Findings, Settings) are
// always reachable via the persistent TopNav (PR #41) — the
// breadcrumb is for SUB-pages that nest below those destinations.

import { useT } from "@/hooks/useT";

type Props = {
  /** Localized name of the destination the back button returns to.
   *  Composed into "Back to {destination}" via the
   *  `common.back_to` i18n template. Examples: t("nav.settings"),
   *  t("nav.dashboard"). */
  destination: string;
  onBack: () => void;
  /** Optional data-testid passthrough; caller-specific so each
   *  page's back button can be targeted independently in tests. */
  "data-testid"?: string;
};

export default function BackBreadcrumb({
  destination,
  onBack,
  "data-testid": testId,
}: Props) {
  const t = useT();
  return (
    <button
      type="button"
      onClick={onBack}
      data-testid={testId ?? "back-breadcrumb"}
      className="inline-flex items-center gap-1.5 text-small font-medium text-saw-grey-700 transition hover:text-saw-red focus-visible:outline focus-visible:outline-2 focus-visible:outline-saw-red"
    >
      <svg
        aria-hidden="true"
        viewBox="0 0 20 20"
        fill="none"
        className="h-3.5 w-3.5"
      >
        <path
          d="M12 4L6 10L12 16"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      </svg>
      {t("common.back_to").replace("{destination}", destination)}
    </button>
  );
}
