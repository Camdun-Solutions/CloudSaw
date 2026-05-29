// PR #80 — Right-side slide-out drawer.
//
// Built specifically for the Findings view's "show finding detail
// without dropping the user out of the per-service browse list"
// requirement: an inline-expand collapse below each row both pushed
// the next row down by ~600px and hid the panel as soon as another
// row was clicked. The drawer pattern keeps the row list intact on
// the left and pins the detail panel on the right.
//
// Behavior mirrors `Modal.tsx` where it can (Escape to close, body
// scroll lock, backdrop click, focus trap on the dismiss button)
// and diverges on layout only — the panel slides from the right
// edge, takes a fixed width on desktop, and goes full-width on
// narrow viewports.
//
// This is not a reimplementation of Modal — Modal stays the
// affordance for confirm/yes-no / form / disclosure dialogs that
// SHOULD command the user's full attention. The drawer is for
// "details about a thing the user clicked, while the page they
// clicked from remains the primary surface."

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
  /** Width preset. `md` (default, 28rem) matches the page-right
   * column on the Findings view; `lg` (40rem) is for content-dense
   * panels like the AI suggestion modal flow. */
  size?: "md" | "lg";
  "data-testid"?: string;
};

/** Body-scroll-lock counter — mirrors Modal's so a drawer and a
 * modal opened on top of each other don't unlock the body when
 * either one closes. */
let openCount = 0;

function lock() {
  if (openCount === 0) {
    document.body.style.overflow = "hidden";
  }
  openCount += 1;
}

function unlock() {
  openCount = Math.max(0, openCount - 1);
  if (openCount === 0) {
    document.body.style.overflow = "";
  }
}

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
  size = "md",
  ...rest
}: Props) {
  const t = useT();
  const closeBtnRef = useRef<HTMLButtonElement>(null);

  // Mirror Modal: lock body scroll while open, restore on close.
  useEffect(() => {
    if (!open) return;
    lock();
    return () => unlock();
  }, [open]);

  // Escape to close + focus the dismiss button on open so the
  // first Tab lands somewhere useful and Esc works from the
  // moment the panel paints.
  useEffect(() => {
    if (!open) return;
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    }
    document.addEventListener("keydown", onKey);
    // requestAnimationFrame so the focus call lands after the slide
    // animation begins — focusing before paint sometimes loses the
    // ring on Chrome.
    const handle = window.requestAnimationFrame(() => {
      closeBtnRef.current?.focus();
    });
    return () => {
      document.removeEventListener("keydown", onKey);
      window.cancelAnimationFrame(handle);
    };
  }, [open, onClose]);

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50"
      role="dialog"
      aria-modal="true"
      aria-label={title}
      {...rest}
    >
      {/* Backdrop. Click to close. Sits below the panel via z-stack
          so a click anywhere outside the panel dismisses. */}
      <div
        className="absolute inset-0 bg-saw-black/40"
        onClick={onClose}
        data-testid="drawer-backdrop"
      />
      {/* The panel itself — slides in from the right via a CSS
          translate transition. `motion-safe:` so users with
          `prefers-reduced-motion` skip the animation. Full-height,
          fixed width at md+ breakpoints, full-width below. */}
      <div
        className={[
          "absolute inset-y-0 right-0 flex w-full flex-col",
          SIZES[size],
          "border-l border-saw-grey-200 dark:border-saw-grey-700",
          "bg-saw-white dark:bg-saw-grey-dark shadow-2xl",
          "motion-safe:transition-transform motion-safe:duration-200",
        ].join(" ")}
        // Stop the backdrop's onClick from firing when the user
        // clicks inside the panel.
        onClick={(e) => e.stopPropagation()}
      >
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
    </div>
  );
}
