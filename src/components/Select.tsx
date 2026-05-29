// PR #77 — Modern custom-dropdown Select.
//
// Replaces the previous native-`<select>` implementation with a custom
// button + floating panel that matches the visual language of:
//   * Text-field-style trigger (rounded-card border, opaque bg, focus
//     ring) — identical to `<input>` elements elsewhere so a form full
//     of mixed text + select fields reads as one consistent surface.
//   * TagInput-style popup (rounded-card panel, rounded suggestion
//     rows, hover state) — every dropdown in the app now shares the
//     same visual idiom regardless of single- vs multi-select.
//
// Why custom rather than native:
//   The native `<select>` element opens an OS-rendered menu whose
//   style is browser/OS-controlled. On CloudSaw's modern dark theme it
//   read as a foreign chrome surface vs the rest of the form. This
//   component is keyboard-accessible (Space / Enter to open, arrows
//   to navigate, Enter to select, Escape to close), respects
//   `disabled`, and uses an outside-click handler to dismiss.

import {
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
} from "react";

type Option<V extends string> = {
  value: V;
  label: string;
};

type SelectProps<V extends string> = {
  label: string;
  value: V;
  options: Option<V>[];
  onChange: (next: V) => void;
  description?: string;
  disabled?: boolean;
  className?: string;
  /** Optional placeholder text shown when no option matches `value`
   * (e.g. when value is the empty string). Defaults to "Select…". */
  placeholder?: string;
  "data-testid"?: string;
};

export default function Select<V extends string>({
  label,
  value,
  options,
  onChange,
  description,
  disabled = false,
  className = "",
  placeholder,
  ...rest
}: SelectProps<V>) {
  const reactId = useId();
  const selectId = `sel-${reactId}`;
  const descId = `${selectId}-desc`;
  const listboxId = `${selectId}-listbox`;

  const [open, setOpen] = useState(false);
  // `activeIndex` is the keyboard-focused row inside the popup —
  // distinct from the currently-selected value. Initialized to the
  // current value's index when the popup opens.
  const [activeIndex, setActiveIndex] = useState<number>(-1);

  const buttonRef = useRef<HTMLButtonElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  const selected = options.find((o) => o.value === value);
  const triggerLabel = selected?.label ?? placeholder ?? "Select…";

  // Sync the keyboard cursor to the selected value whenever the
  // popup opens so arrow-key nav starts from a sensible spot.
  useEffect(() => {
    if (open) {
      const idx = options.findIndex((o) => o.value === value);
      setActiveIndex(idx >= 0 ? idx : 0);
    }
  }, [open, value, options]);

  // Outside-click + Escape close. The handler runs only while the
  // popup is open to keep the listener count tiny.
  useEffect(() => {
    if (!open) return;
    function onDocClick(e: MouseEvent) {
      if (!containerRef.current) return;
      if (!containerRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    }
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") {
        e.stopPropagation();
        setOpen(false);
        buttonRef.current?.focus();
      }
    }
    document.addEventListener("mousedown", onDocClick);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDocClick);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const commit = useCallback(
    (idx: number) => {
      const opt = options[idx];
      if (!opt) return;
      onChange(opt.value);
      setOpen(false);
      // Return focus to the trigger so subsequent keyboard nav lands
      // on something sensible.
      buttonRef.current?.focus();
    },
    [options, onChange],
  );

  function onTriggerKeyDown(e: React.KeyboardEvent<HTMLButtonElement>) {
    if (disabled) return;
    if (e.key === "ArrowDown" || e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      setOpen(true);
    }
  }

  function onListKeyDown(e: React.KeyboardEvent<HTMLDivElement>) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActiveIndex((i) => Math.min(options.length - 1, i + 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActiveIndex((i) => Math.max(0, i - 1));
    } else if (e.key === "Home") {
      e.preventDefault();
      setActiveIndex(0);
    } else if (e.key === "End") {
      e.preventDefault();
      setActiveIndex(options.length - 1);
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (activeIndex >= 0) commit(activeIndex);
    }
  }

  return (
    <div className={`flex flex-col gap-1.5 ${className}`} ref={containerRef}>
      <label
        htmlFor={selectId}
        className="text-small font-medium text-saw-grey-700 dark:text-saw-grey-300"
      >
        {label}
      </label>
      <div className="relative">
        <button
          ref={buttonRef}
          id={selectId}
          type="button"
          role="combobox"
          aria-haspopup="listbox"
          aria-expanded={open}
          aria-controls={listboxId}
          aria-describedby={description ? descId : undefined}
          disabled={disabled}
          onClick={() => !disabled && setOpen((v) => !v)}
          onKeyDown={onTriggerKeyDown}
          {...rest}
          className={[
            "flex w-full items-center justify-between gap-3 rounded-card border px-3 py-2",
            "text-left text-body",
            disabled
              ? "cursor-not-allowed border-saw-grey-200 dark:border-saw-grey-700 bg-saw-grey-50 dark:bg-saw-grey-800 text-saw-grey-500 dark:text-saw-grey-400"
              : "border-saw-grey-300 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark text-saw-grey-900 dark:text-saw-beige",
            "focus:outline-none focus:ring-2 focus:ring-saw-orange focus:ring-offset-1",
          ].join(" ")}
        >
          <span
            className={
              selected
                ? "truncate"
                : "truncate text-saw-grey-500 dark:text-saw-grey-400"
            }
          >
            {triggerLabel}
          </span>
          {/* Chevron — rotates when open. */}
          <svg
            aria-hidden="true"
            viewBox="0 0 12 12"
            className={[
              "h-3 w-3 shrink-0 text-saw-grey-500 transition-transform",
              open ? "rotate-180" : "",
            ].join(" ")}
          >
            <path
              d="M2 4l4 4 4-4"
              stroke="currentColor"
              strokeWidth="1.5"
              fill="none"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
        </button>

        {open ? (
          <div
            id={listboxId}
            role="listbox"
            aria-activedescendant={
              activeIndex >= 0 ? `${selectId}-opt-${activeIndex}` : undefined
            }
            tabIndex={-1}
            // autoFocus so arrow-key nav works immediately on open.
            ref={(el) => {
              el?.focus();
            }}
            onKeyDown={onListKeyDown}
            className="absolute left-0 right-0 top-full z-30 mt-1 max-h-60 overflow-y-auto rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark py-1 shadow-lg focus:outline-none"
          >
            {options.map((o, idx) => {
              const isSelected = o.value === value;
              const isActive = idx === activeIndex;
              return (
                <button
                  key={o.value}
                  id={`${selectId}-opt-${idx}`}
                  type="button"
                  role="option"
                  aria-selected={isSelected}
                  onMouseEnter={() => setActiveIndex(idx)}
                  // mousedown beats the trigger's blur handler so the
                  // popup commits before re-closing on outside-click.
                  onMouseDown={(e) => {
                    e.preventDefault();
                    commit(idx);
                  }}
                  className={[
                    "flex w-full items-center justify-between gap-2 px-3 py-1.5 text-left text-small",
                    isActive
                      ? "bg-saw-grey-100 dark:bg-saw-grey-800"
                      : "hover:bg-saw-grey-100 dark:hover:bg-saw-grey-800",
                    isSelected
                      ? "font-medium text-saw-grey-900 dark:text-saw-beige"
                      : "text-saw-grey-800 dark:text-saw-beige",
                    "focus:outline-none",
                  ].join(" ")}
                >
                  <span className="truncate">{o.label}</span>
                  {isSelected ? (
                    <svg
                      aria-hidden="true"
                      viewBox="0 0 12 12"
                      className="h-3 w-3 shrink-0 text-saw-red"
                    >
                      <path
                        d="M2.5 6.5l2.5 2.5L9.5 3.5"
                        stroke="currentColor"
                        strokeWidth="1.75"
                        fill="none"
                        strokeLinecap="round"
                        strokeLinejoin="round"
                      />
                    </svg>
                  ) : null}
                </button>
              );
            })}
          </div>
        ) : null}
      </div>
      {description ? (
        <p id={descId} className="text-small text-saw-grey-500 dark:text-saw-grey-400">
          {description}
        </p>
      ) : null}
    </div>
  );
}
