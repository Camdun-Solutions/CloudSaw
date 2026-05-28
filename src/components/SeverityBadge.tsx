// SeverityBadge — a severity indicator that is NEVER conveyed by color alone.
// Every render carries the severity word (localized), an SR-only role label,
// and a geometric glyph in addition to the colored swatch, so a color-blind
// user (or a high-contrast OS theme) can still distinguish severities.
// Contract 09 §Constraints + CLAUDE.md §4.6.

import { useT } from "@/hooks/useT";
import type { Severity } from "@/lib/ipc";

type Props = {
  severity: Severity;
  size?: "sm" | "md";
  /** When true, render only the glyph + sr-only text. The visible label is
   * dropped so callers can pack severity into dense grids without losing
   * the accessibility guarantee. */
  iconOnly?: boolean;
};

type Tone = {
  /** Wrapper background + text color. */
  wrapper: string;
  /** Glyph fill color. */
  glyph: string;
  /** Unicode geometric shape — a redundant non-color channel. */
  shape: string;
};

const TONES: Record<Severity, Tone> = {
  critical: {
    wrapper: "bg-saw-grey-900 text-saw-white",
    glyph: "bg-saw-white",
    // Filled square: visually densest, signals "highest".
    shape: "■",
  },
  high: {
    wrapper: "bg-saw-red/10 text-saw-grey-900 dark:text-saw-beige border border-saw-red/40",
    glyph: "bg-saw-red",
    shape: "▲",
  },
  medium: {
    wrapper: "bg-saw-orange/10 text-saw-grey-900 dark:text-saw-beige border border-saw-orange/40",
    glyph: "bg-saw-orange",
    shape: "◆",
  },
  low: {
    wrapper: "bg-saw-gold/10 text-saw-grey-900 dark:text-saw-beige border border-saw-gold/40",
    glyph: "bg-saw-gold",
    shape: "●",
  },
  informational: {
    wrapper: "bg-saw-grey-100 dark:bg-saw-grey-800 text-saw-grey-800 dark:text-saw-beige border border-saw-grey-300 dark:border-saw-grey-700",
    glyph: "bg-saw-grey-400",
    shape: "○",
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
      <span aria-hidden="true" className="text-saw-grey-700 dark:text-saw-grey-300">
        {tone.shape}
      </span>
      {iconOnly ? (
        <span className="sr-only">{label}</span>
      ) : (
        <span>{label}</span>
      )}
    </span>
  );
}
