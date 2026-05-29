import { useId } from "react";

type SwitchProps = {
  checked: boolean;
  onChange: (next: boolean) => void;
  label: string;
  description?: string;
  disabled?: boolean;
  disabledReason?: string;
};

// Toggle that conveys state with shape + color + a screen-reader-friendly
// label so accessibility doesn't depend on the indicator color alone
// (CLAUDE.md §4.6).
export default function Switch({
  checked,
  onChange,
  label,
  description,
  disabled = false,
  disabledReason,
}: SwitchProps) {
  const reactId = useId();
  const descId = `${reactId}-desc`;
  const help = disabled && disabledReason ? disabledReason : description;

  return (
    <div className="flex items-start justify-between gap-4">
      <div className="flex-1">
        <p
          className={[
            "text-body font-medium",
            disabled ? "text-saw-grey-500 dark:text-saw-grey-400" : "text-saw-grey-900 dark:text-saw-beige",
          ].join(" ")}
        >
          {label}
        </p>
        {help ? (
          <p id={descId} className="mt-0.5 text-small text-saw-grey-500 dark:text-saw-grey-400">
            {help}
          </p>
        ) : null}
      </div>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        aria-label={label}
        aria-describedby={help ? descId : undefined}
        disabled={disabled}
        onClick={() => onChange(!checked)}
        className={[
          "relative inline-flex h-6 w-11 shrink-0 items-center rounded-full transition-colors",
          "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-saw-orange",
          "focus-visible:ring-offset-2",
          // PR #77 — the off-state previously fell back to a light-
          // grey track on both themes (`bg-saw-grey-300`), which read
          // as a pale band against the dark canvas. The user wanted a
          // dark fill + light outline in dark mode so the off-state
          // reads as a recessed shape rather than a stray light
          // patch. Light theme keeps the existing solid grey track.
          disabled
            ? "cursor-not-allowed bg-saw-grey-200 dark:bg-saw-grey-800 dark:border dark:border-saw-grey-600"
            : checked
              ? "bg-saw-red"
              : "bg-saw-grey-300 dark:border dark:border-saw-grey-500 dark:bg-saw-grey-dark",
        ].join(" ")}
      >
        <span
          aria-hidden="true"
          className={[
            "inline-block h-5 w-5 transform rounded-full bg-saw-white shadow transition-transform",
            checked ? "translate-x-5" : "translate-x-0.5",
          ].join(" ")}
        />
      </button>
    </div>
  );
}
