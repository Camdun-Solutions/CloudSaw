import { forwardRef, useId, useState, type InputHTMLAttributes } from "react";

type PasswordFieldProps = Omit<InputHTMLAttributes<HTMLInputElement>, "type"> & {
  label: string;
  /** Optional inline error message shown beneath the field. */
  error?: string | null;
  /** Optional helper copy shown beneath the field when no error is present. */
  hint?: string | null;
  /** Localized labels for the show/hide button. */
  showLabel?: string;
  hideLabel?: string;
};

const PasswordField = forwardRef<HTMLInputElement, PasswordFieldProps>(
  function PasswordField(
    {
      label,
      error,
      hint,
      showLabel = "Show",
      hideLabel = "Hide",
      className = "",
      id,
      autoComplete = "current-password",
      ...rest
    },
    ref,
  ) {
    const reactId = useId();
    const inputId = id ?? `pw-${reactId}`;
    const hintId = `${inputId}-hint`;
    const errorId = `${inputId}-error`;
    const [revealed, setRevealed] = useState(false);

    return (
      <div className={`flex flex-col gap-1.5 ${className}`}>
        <label
          htmlFor={inputId}
          className="text-small font-medium text-saw-grey-700"
        >
          {label}
        </label>
        <div className="relative">
          <input
            ref={ref}
            id={inputId}
            type={revealed ? "text" : "password"}
            autoComplete={autoComplete}
            spellCheck={false}
            aria-invalid={error ? true : undefined}
            aria-describedby={error ? errorId : hint ? hintId : undefined}
            className={[
              "block w-full rounded-card border bg-saw-white px-3 py-2 pr-16 text-body",
              "text-saw-grey-900 placeholder:text-saw-grey-400",
              "focus:outline-none focus:ring-2 focus:ring-saw-orange focus:ring-offset-1",
              error
                ? "border-saw-red focus:ring-saw-red"
                : "border-saw-grey-300",
            ].join(" ")}
            {...rest}
          />
          <button
            type="button"
            onClick={() => setRevealed((r) => !r)}
            aria-pressed={revealed}
            className={[
              "absolute inset-y-0 right-0 flex items-center px-3 text-small font-medium",
              "text-saw-grey-600 hover:text-saw-grey-900",
              "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-saw-orange",
            ].join(" ")}
          >
            {revealed ? hideLabel : showLabel}
          </button>
        </div>
        {error ? (
          <p id={errorId} className="text-small text-saw-red">
            {error}
          </p>
        ) : hint ? (
          <p id={hintId} className="text-small text-saw-grey-500">
            {hint}
          </p>
        ) : null}
      </div>
    );
  },
);

export default PasswordField;
