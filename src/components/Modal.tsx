import { useEffect, useRef, type ReactNode } from "react";

import { useT } from "@/hooks/useT";

type ModalProps = {
  open: boolean;
  onClose: () => void;
  title: string;
  children: ReactNode;
  footer?: ReactNode;
};

export default function Modal({
  open,
  onClose,
  title,
  children,
  footer,
}: ModalProps) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const t = useT();

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKey);
    // Move focus into the dialog so keyboard users land inside it.
    dialogRef.current?.focus();
    return () => document.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-saw-black/40 p-4"
      role="presentation"
      onClick={onClose}
    >
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="modal-title"
        tabIndex={-1}
        className="w-full max-w-lg rounded-card bg-saw-white shadow-xl outline-none"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-3 border-b border-saw-grey-200 px-5 py-3">
          <h2
            id="modal-title"
            className="flex-1 text-h3 font-semibold text-saw-grey-900"
          >
            {title}
          </h2>
          {/* Explicit close affordance — the modal already closes on
              Escape and backdrop click, but users (especially on touch
              devices, or inside a Tauri webview where Escape may be
              consumed by an upstream listener like a focused input)
              need a visible target. Sized 32x32 so the click target
              meets WCAG 2.2 AA minimum 24x24 with margin to spare. */}
          <button
            type="button"
            onClick={onClose}
            aria-label={t("common.close")}
            data-testid="modal-close-button"
            className="-mr-1 inline-flex h-8 w-8 items-center justify-center rounded-full text-saw-grey-500 transition hover:bg-saw-grey-100 hover:text-saw-grey-800 focus-visible:outline focus-visible:outline-2 focus-visible:outline-saw-red"
          >
            <svg
              aria-hidden="true"
              viewBox="0 0 20 20"
              fill="none"
              className="h-4 w-4"
            >
              <path
                d="M5 5L15 15M15 5L5 15"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
              />
            </svg>
          </button>
        </div>
        <div className="px-5 py-4 text-body text-saw-grey-800">{children}</div>
        {footer ? (
          <div className="flex justify-end gap-2 border-t border-saw-grey-200 px-5 py-3">
            {footer}
          </div>
        ) : null}
      </div>
    </div>
  );
}
