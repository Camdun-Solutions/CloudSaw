import type { HTMLAttributes, ReactNode } from "react";

type Tone = "neutral" | "info" | "success" | "warning" | "danger";

type BadgeProps = HTMLAttributes<HTMLSpanElement> & {
  tone?: Tone;
  children: ReactNode;
};

// Badges encode severity with BOTH color and a leading dot/glyph so the
// information is not conveyed by color alone (CLAUDE.md §4.6).
const tones: Record<Tone, { wrapper: string; dot: string; label: string }> = {
  neutral: {
    wrapper: "bg-saw-grey-100 dark:bg-saw-grey-800 text-saw-grey-700 dark:text-saw-grey-300",
    dot: "bg-saw-grey-400",
    label: "Neutral",
  },
  info: {
    wrapper: "bg-saw-grey-100 dark:bg-saw-grey-800 text-saw-grey-800 dark:text-saw-beige",
    dot: "bg-saw-gold",
    label: "Info",
  },
  success: {
    wrapper: "bg-saw-grey-100 dark:bg-saw-grey-800 text-saw-grey-800 dark:text-saw-beige",
    dot: "bg-emerald-500",
    label: "OK",
  },
  warning: {
    wrapper: "bg-saw-grey-100 dark:bg-saw-grey-800 text-saw-grey-900 dark:text-saw-beige",
    dot: "bg-saw-orange",
    label: "Warning",
  },
  danger: {
    wrapper: "bg-saw-grey-100 dark:bg-saw-grey-800 text-saw-grey-900 dark:text-saw-beige",
    dot: "bg-saw-red",
    label: "Danger",
  },
};

export default function Badge({
  tone = "neutral",
  className = "",
  children,
  ...rest
}: BadgeProps) {
  const t = tones[tone];
  const cls = [
    "inline-flex items-center gap-1.5 rounded-full px-2.5 py-0.5 text-small font-medium",
    t.wrapper,
    className,
  ]
    .filter(Boolean)
    .join(" ");
  return (
    <span className={cls} {...rest}>
      <span
        className={`h-1.5 w-1.5 rounded-full ${t.dot}`}
        aria-hidden="true"
      />
      <span className="sr-only">{t.label}:</span>
      {children}
    </span>
  );
}
