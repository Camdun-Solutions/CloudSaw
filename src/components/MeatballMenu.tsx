// Compact "vertical meatball" menu (⋮) for per-row action overflow.
//
// PR #66 introduces this so the Accounts row can collapse its Edit /
// Scan / Re-configure / Delete buttons into a single anchor button
// without overflowing narrow Settings panels. The component is
// generic — any caller passes a list of items with their handlers.
//
// Behavior:
//   - Click the ⋮ trigger to toggle the dropdown.
//   - Click outside, click an item, or press Escape to close.
//   - Items render in order; the last item can be marked `danger` to
//     style it in saw-red (typical for destructive actions like Delete).
//   - Items are real <button> elements inside role="menu" / role="menuitem"
//     containers, so screen readers announce them correctly.

import {
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
  type ReactNode,
} from "react";

export type MeatballMenuItem = {
  /** Visible label. */
  label: string;
  /** Click handler. The menu auto-closes after firing. */
  onClick: () => void;
  /** When true, the item is styled in saw-red — reserved for
   *  destructive actions (Delete / Remove). */
  danger?: boolean;
  /** When true, the item is rendered but un-clickable. */
  disabled?: boolean;
  /** Optional leading glyph. Plain ReactNode (no icon library
   *  dependency). */
  icon?: ReactNode;
  /** Stable test id suffix; the menu prepends a known prefix. */
  testId?: string;
};

type Props = {
  items: MeatballMenuItem[];
  /** Aria-label for the trigger button. Defaults to "More actions". */
  triggerLabel?: string;
  /** Stable test id for the trigger button. */
  triggerTestId?: string;
};

export default function MeatballMenu({
  items,
  triggerLabel = "More actions",
  triggerTestId,
}: Props) {
  const [open, setOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuId = useId();

  const close = useCallback(() => setOpen(false), []);

  useEffect(() => {
    if (!open) return;
    function onDocClick(e: MouseEvent) {
      if (!containerRef.current?.contains(e.target as Node)) close();
    }
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") {
        close();
        triggerRef.current?.focus();
      }
    }
    document.addEventListener("mousedown", onDocClick);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDocClick);
      document.removeEventListener("keydown", onKey);
    };
  }, [open, close]);

  return (
    <div ref={containerRef} className="relative inline-block">
      <button
        ref={triggerRef}
        type="button"
        onClick={() => setOpen((o) => !o)}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={menuId}
        aria-label={triggerLabel}
        title={triggerLabel}
        data-testid={triggerTestId}
        className="inline-flex h-8 w-8 items-center justify-center rounded-full text-saw-grey-600 dark:text-saw-grey-300 hover:bg-saw-grey-100 dark:hover:bg-saw-grey-800 focus:outline-none focus-visible:ring-2 focus-visible:ring-saw-red"
      >
        <span aria-hidden="true" className="text-lg leading-none">⋮</span>
      </button>
      {open ? (
        <div
          id={menuId}
          role="menu"
          aria-label={triggerLabel}
          className="absolute right-0 z-20 mt-1 min-w-[12rem] overflow-hidden rounded-card border border-saw-grey-200 dark:border-saw-grey-700 bg-saw-white dark:bg-saw-grey-dark shadow-lg"
        >
          <ul className="flex flex-col py-1">
            {items.map((item, idx) => (
              <li key={idx} role="none">
                <button
                  type="button"
                  role="menuitem"
                  disabled={item.disabled}
                  data-testid={item.testId}
                  onClick={() => {
                    if (item.disabled) return;
                    close();
                    item.onClick();
                  }}
                  className={
                    "flex w-full items-center gap-2 px-3 py-2 text-left text-small transition disabled:cursor-not-allowed disabled:opacity-50 " +
                    (item.danger
                      ? "text-saw-red hover:bg-saw-red/10"
                      : "text-saw-grey-900 dark:text-saw-beige hover:bg-saw-grey-100 dark:hover:bg-saw-grey-800")
                  }
                >
                  {item.icon ? (
                    <span aria-hidden="true" className="flex-shrink-0">
                      {item.icon}
                    </span>
                  ) : null}
                  <span className="flex-1">{item.label}</span>
                </button>
              </li>
            ))}
          </ul>
        </div>
      ) : null}
    </div>
  );
}
