// PR #80 — Right-side slide-out drawer.
//
// Built specifically for the Findings view's "show finding detail
// without dropping the user out of the per-service browse list"
// requirement: an inline-expand collapse below each row both pushed
// the next row down by ~600px and hid the panel as soon as another
// row was clicked. The drawer pattern keeps the row list intact on
// the left and pins the detail panel on the right.
//
// PR #83 — Non-modal. The earlier modal behavior (backdrop dim,
// body scroll lock, click-outside-to-close, aria-modal=true) forced
// the user to dismiss the drawer before they could click another
// finding row, which defeats the whole "browse list stays usable"
// motivation. The drawer now floats over the right edge without
// covering or blocking the underlying page: no backdrop, no scroll
// lock, list rows behind the right column stay clickable. Dismiss
// is the X button or Escape.
//
// Modal.tsx remains the affordance for true command-the-user
// dialogs (confirm/yes-no / form / disclosure).

import { useEffect, useRef, type ReactNode } from "react";

import { useT } from "@/hooks/useT";

type Props = {
  open: boolean;
  onClose: () => void;
  title: string;
  /** Optional subtitle rendered under the title. Useful for
   * surfacing the parent context (e.g. the service name when the
   * drawer holds a finding). */
  subtitle?: string;
  children: ReactNode;
  /** Footer rendered as a sticky bottom row inside the drawer. */
  footer?: ReactNode;
  /** PR #81 — Optional action slot rendered in the header row, between
   * the title block and the dismiss button. Used by the Findings drawer
   * to surface the "Create GitHub Issue" affordance at the top-right of
   * the panel (so the action is reachable without scrolling the body). */
  headerAction?: ReactNode;
  /** Width preset. `md` (default, 28rem) matches the page-right
   * column on the Findings view; `lg` (40rem) is for content-dense
   * panels like the AI suggestion modal flow. */
  size?: "md" | "lg";
  "data-testid"?: string;
};

// PR #83 — Body scroll lock removed. Non-modal drawer should not
// freeze the underlying page; the user is supposed to keep browsing
// the list while the drawer floats on the right.

const SIZES: Record<NonNullable<Props["size"]>, string> = {
  md: "max-w-md",
  lg: "max-w-2xl",
};

export default function Drawer({
  open,
  onClose,
  title,
  subtitle,
  children,
  footer,
  headerAction,
  size = "md",
  ...rest
}: Props) {
  const t = useT();
  const closeBtnRef = useRef<HTMLButtonElement>(null);

  // PR #83 — Escape closes; no body-scroll lock or backdrop click
  // handler because the drawer no longer covers the page. The
  // focus-on-open behavior is also gone: the user is supposed to
  // keep clicking findings on the list, so stealing focus into the
  // drawer would fight that.
  useEffect(() => {
    if (!open) return;
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    }
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("keydown", onKey);
    };
  }, [open, onClose]);

  if (!open) return null;

  return (
    // PR #83 — Non-modal: role=complementary instead of dialog,
    // no aria-modal. The panel pins to the right edge but the
    // rest of the page (the findings list behind it) stays
    // interactive — no fixed positioned overlay covering inset-0
    // to swallow clicks.
    <aside
      className={[
        "fixed inset-y-0 right-0 z-40 flex w-full flex-col",
        SIZES[size],
        "border-l border-saw-grey-200 dark:border-saw-grey-700",
        "bg-saw-white dark:bg-saw-grey-dark shadow-2xl",
        "motion-safe:transition-transform motion-safe:duration-200",
      ].join(" ")}
      role="complementary"
      aria-label={title}
      {...rest}
    >
      <div className="flex h-full w-full flex-col">
        <header className="flex items-start justify-between gap-3 border-b border-saw-grey-200 dark:border-saw-grey-700 px-5 py-4">
          <div className="min-w-0 flex-1">
            <h2 className="text-h3 font-semibold text-saw-grey-900 dark:text-saw-beige">
              {title}
            </h2>
            {subtitle ? (
              <p className="mt-1 text-small text-saw-grey-600 dark:text-saw-grey-400">
                {subtitle}
              </p>
            ) : null}
          </div>
          {headerAction ? (
            <div
              className="flex shrink-0 items-start"
              data-testid="drawer-header-action"
            >
              {headerAction}
            </div>
          ) : null}
          <button
            ref={closeBtnRef}
            type="button"
            onClick={onClose}
            aria-label={t("common.close")}
            data-testid="drawer-close"
            className="-mr-1 inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-card text-saw-grey-600 hover:bg-saw-grey-100 dark:text-saw-grey-300 dark:hover:bg-saw-grey-800 focus:outline-none focus-visible:ring-2 focus-visible:ring-saw-orange"
          >
            <svg viewBox="0 0 16 16" className="h-4 w-4" aria-hidden="true">
              <path
                d="M3 3l10 10M13 3L3 13"
                stroke="currentColor"
                strokeWidth="1.75"
                strokeLinecap="round"
                fill="none"
              />
            </svg>
          </button>
        </header>
        <div className="flex-1 overflow-y-auto px-5 py-4">{children}</div>
        {footer ? (
          <footer className="border-t border-saw-grey-200 dark:border-saw-grey-700 px-5 py-3">
            {footer}
          </footer>
        ) : null}
      </div>
    </aside>
  );
}
