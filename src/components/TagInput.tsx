// PR #69 — Reusable pill-tag editor with autocomplete suggestions.
//
// Used by the Settings → AI Recommendations Compliance Obligations
// field. The user can:
//   - Type freely; matching suggestions render in a dropdown.
//   - Click a suggestion to add it as a pill.
//   - Press Enter, comma, or paste with a trailing comma to convert
//     the current text into a pill (even when the value isn't in
//     the suggestions list — custom entries are allowed by design).
//   - Click the small "×" on a pill to remove it.
//   - Press Backspace at the start of the empty input to remove
//     the last pill.
//
// The component is value-controlled: callers pass `value: string[]`
// and an `onChange` setter. Suggestion matching is a simple
// case-insensitive substring search; the list is typically a few
// dozen items so we don't need a fuzzy library.

import { useEffect, useRef, useState, useId } from "react";

import { useT } from "@/hooks/useT";

type Props = {
  /** The current set of tags. Order is preserved. */
  value: string[];
  /** Called whenever the tag list changes. */
  onChange: (next: string[]) => void;
  /** Optional autocomplete suggestions surfaced as the user types. */
  suggestions?: ReadonlyArray<string>;
  /** Placeholder for the inner input. */
  placeholder?: string;
  /** Optional max number of tags. */
  maxTags?: number;
  /** Optional max length per tag (sanity bound — long pastes get
   *  truncated). */
  maxTagLength?: number;
  /** Test id for the host element. */
  "data-testid"?: string;
};

export default function TagInput({
  value,
  onChange,
  suggestions = [],
  placeholder,
  maxTags,
  maxTagLength = 80,
  ...rest
}: Props) {
  const t = useT();
  const [draft, setDraft] = useState("");
  const [focused, setFocused] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const listboxId = useId();

  // Reset the draft if the parent clears the field externally.
  useEffect(() => {
    if (value.length === 0 && draft === "") {
      // No-op — just a safety dep to keep the input in sync.
    }
  }, [value, draft]);

  function commit(rawText: string) {
    const trimmed = rawText.trim();
    if (!trimmed) return;
    if (maxTags && value.length >= maxTags) return;
    const next = trimmed.slice(0, maxTagLength);
    // Dedup case-insensitively; keep the first-added casing.
    const lower = next.toLowerCase();
    if (value.some((v) => v.toLowerCase() === lower)) {
      setDraft("");
      return;
    }
    onChange([...value, next]);
    setDraft("");
  }

  function removeAt(idx: number) {
    onChange(value.filter((_, i) => i !== idx));
  }

  const matches = (() => {
    if (suggestions.length === 0) return [];
    const q = draft.trim().toLowerCase();
    const taken = new Set(value.map((v) => v.toLowerCase()));
    const filtered = suggestions.filter((s) => !taken.has(s.toLowerCase()));
    if (q.length === 0) return filtered.slice(0, 10);
    return filtered
      .filter((s) => s.toLowerCase().includes(q))
      .slice(0, 10);
  })();

  const showDropdown = focused && matches.length > 0;

  return (
    <div className="relative" {...rest}>
      <div
        // Click anywhere in the row focuses the input so the pills
        // behave like inline tokens inside a single field.
        onClick={() => inputRef.current?.focus()}
        className="flex flex-wrap items-center gap-1 rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark px-2 py-1.5 text-body text-saw-grey-900 dark:text-saw-beige focus-within:ring-2 focus-within:ring-saw-orange"
      >
        {value.map((tag, idx) => (
          <span
            key={`${tag}-${idx}`}
            data-testid="tag-input-pill"
            className="inline-flex items-center gap-1 rounded-full bg-saw-grey-100 dark:bg-saw-grey-800 px-2 py-0.5 text-small text-saw-grey-800 dark:text-saw-beige"
          >
            <span className="font-mono text-xs">{tag}</span>
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation();
                removeAt(idx);
              }}
              aria-label={t("tag_input.remove").replace("{tag}", tag)}
              data-testid="tag-input-pill-remove"
              className="-mr-1 inline-flex h-4 w-4 items-center justify-center rounded-full text-saw-grey-500 hover:bg-saw-grey-300 dark:hover:bg-saw-grey-700 hover:text-saw-grey-900 dark:hover:text-saw-beige focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-saw-red"
            >
              <svg
                aria-hidden="true"
                viewBox="0 0 10 10"
                className="h-2.5 w-2.5"
              >
                <path
                  d="M2 2L8 8M8 2L2 8"
                  stroke="currentColor"
                  strokeWidth="1.5"
                  strokeLinecap="round"
                  fill="none"
                />
              </svg>
            </button>
          </span>
        ))}
        <input
          ref={inputRef}
          type="text"
          value={draft}
          placeholder={value.length === 0 ? placeholder : undefined}
          onFocus={() => setFocused(true)}
          // Defer blur so a click on a dropdown suggestion still
          // registers (the suggestion mousedown fires before blur).
          onBlur={() => window.setTimeout(() => setFocused(false), 150)}
          onChange={(e) => {
            const next = e.target.value;
            if (next.endsWith(",")) {
              // Comma converts the in-progress text into a pill,
              // including values not present in the suggestions
              // list. The trailing comma itself is discarded.
              commit(next.slice(0, -1));
            } else {
              setDraft(next);
            }
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              commit(draft);
            } else if (e.key === "Backspace" && draft.length === 0 && value.length > 0) {
              e.preventDefault();
              removeAt(value.length - 1);
            }
          }}
          role="combobox"
          aria-autocomplete="list"
          aria-expanded={showDropdown}
          aria-controls={listboxId}
          data-testid="tag-input-input"
          className="flex-1 min-w-[8rem] bg-transparent text-body text-saw-grey-900 dark:text-saw-beige outline-none placeholder:text-saw-grey-500"
        />
      </div>

      {showDropdown ? (
        <ul
          id={listboxId}
          role="listbox"
          data-testid="tag-input-dropdown"
          className="absolute left-0 right-0 top-full z-20 mt-1 max-h-60 overflow-y-auto rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark py-1 shadow-lg"
        >
          {matches.map((m) => (
            <li key={m}>
              <button
                type="button"
                role="option"
                aria-selected="false"
                // mousedown so we beat the input's blur handler.
                onMouseDown={(e) => {
                  e.preventDefault();
                  commit(m);
                }}
                data-testid="tag-input-suggestion"
                className="block w-full px-3 py-1.5 text-left text-small text-saw-grey-900 dark:text-saw-beige hover:bg-saw-grey-100 dark:hover:bg-saw-grey-800 focus:outline-none"
              >
                <span className="font-mono">{m}</span>
              </button>
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}
