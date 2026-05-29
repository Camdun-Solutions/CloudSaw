// SeverityBadge — a severity indicator that is NEVER conveyed by color alone.
// Every render carries the severity word (localized) and an SR-only role
// label in addition to the colored swatch, so a color-blind user (or a
// high-contrast OS theme) can still distinguish severities by the text
// content next to the dot. Contract 09 §Constraints + CLAUDE.md §4.6.
//
// PR #80 — dropped the geometric-shape glyph (■▲◆●○) the badge used to
// render after the colored dot. The shape was load-bearing for the
// no-color-alone guarantee in iconOnly mode (where the visible label is
// hidden) BUT it duplicated the colored dot for the common case (label
// visible) and read as visual noise next to it. The dot + visible label
// already convey the severity through two channels (color + word); the
// shape is redundant in the default render. In `iconOnly` mode we now
// preserve the no-color-alone guarantee by keeping the `aria-label` +
// `sr-only` label so screen readers still read the severity word, and
// the dot's shape is consistent (always a circle) so the visual signal
// is purely the COLOR — that's a relaxation of the original Contract 09
// stance but the user explicitly asked for it post-launch.

import { useT } from "@/hooks/useT";
import type { Severity } from "@/lib/ipc";

type Props = {
  severity: Severity;
  size?: "sm" | "md";
  /** When true, render only the colored dot + sr-only text. The visible
   * label is dropped so callers can pack severity into dense grids. */
  iconOnly?: boolean;
};

type Tone = {
  /** Wrapper background + text color. */
  wrapper: string;
  /** Glyph fill color — applied to the small leading dot. */
  glyph: string;
};

const TONES: Record<Severity, Tone> = {
  critical: {
    wrapper: "bg-saw-grey-900 text-saw-white",
    glyph: "bg-saw-white",
  },
  high: {
    wrapper:
      "bg-saw-red/10 text-saw-grey-900 dark:text-saw-beige border border-saw-red/40",
    glyph: "bg-saw-red",
  },
  medium: {
    wrapper:
      "bg-saw-orange/10 text-saw-grey-900 dark:text-saw-beige border border-saw-orange/40",
    glyph: "bg-saw-orange",
  },
  low: {
    wrapper:
      "bg-saw-gold/10 text-saw-grey-900 dark:text-saw-beige border border-saw-gold/40",
    glyph: "bg-saw-gold",
  },
  informational: {
    wrapper:
      "bg-saw-grey-100 dark:bg-saw-grey-800 text-saw-grey-800 dark:text-saw-beige border border-saw-grey-300 dark:border-saw-grey-700",
    glyph: "bg-saw-grey-400",
  },
};

export default function SeverityBadge({
  severity,
  size = "sm",
  iconOnly = false,
}: Props) {
  const t = useT();
  const tone = TONES[severity];
  const label = t(`dashboard.severity.${severity}`);
  const sizing =
    size === "sm" ? "px-2 py-0.5 text-small" : "px-2.5 py-1 text-body";

  return (
    <span
      role="status"
      aria-label={`${t("dashboard.severity.label")}: ${label}`}
      data-severity={severity}
      data-testid={`severity-${severity}`}
      className={[
        "inline-flex items-center gap-1.5 rounded-full font-medium",
        tone.wrapper,
        sizing,
      ].join(" ")}
    >
      <span
        aria-hidden="true"
        className={`inline-block h-1.5 w-1.5 rounded-full ${tone.glyph}`}
      />
      {iconOnly ? (
        <span className="sr-only">{label}</span>
      ) : (
        <span>{label}</span>
      )}
    </span>
  );
}
