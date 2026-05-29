import { forwardRef, type ButtonHTMLAttributes } from "react";

type Variant = "primary" | "secondary" | "ghost" | "danger";

// PR #55: added `lg` for marquee CTAs ("Scan Now", "Run scan",
// onboarding primary actions). The hierarchy is sm (compact / inline
// toolbars) → md (default) → lg (the single dominant action on a view).
type Size = "sm" | "md" | "lg";

type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: Variant;
  size?: Size;
};

// PR #77 — `whitespace-nowrap` added to base so multi-word labels
// like "Add provider" never break onto two lines when the parent
// container narrows (e.g. side-by-side header chrome with a tight
// "Connected providers · Add provider" row). Buttons are sized by
// content width by default and were inheriting the wrapping
// behavior of their containing flexbox. Coupled with `shrink-0`
// so a flex container can't compress the button below its
// intrinsic content width either.
const base =
  "inline-flex items-center justify-center gap-2 whitespace-nowrap shrink-0 " +
  "rounded-card font-medium " +
  "transition-colors disabled:cursor-not-allowed disabled:opacity-60 " +
  "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-saw-orange " +
  "focus-visible:ring-offset-2 focus-visible:ring-offset-saw-grey-50 " +
  "dark:focus-visible:ring-offset-saw-black";

const variants: Record<Variant, string> = {
  // PR #55: primary CTAs use the bolder saw-red-bold token so the
  // dominant action reads as the strongest element on the page.
  // Hover/active states stay on the standard saw-red ramp. PR #57:
  // saw-red carries through both modes — accent doesn't shift in
  // dark mode.
  primary:
    "bg-saw-red-bold text-saw-white hover:bg-saw-red active:bg-saw-red/90",
  // PR #57: dark-mode secondary surface — beige text on grey-dark
  // panel, with the same border + hover ramp inverted.
  secondary:
    "bg-saw-white text-saw-grey-900 border border-saw-grey-300 " +
    "hover:bg-saw-grey-100 active:bg-saw-grey-200 " +
    "dark:bg-saw-grey-dark dark:text-saw-beige dark:border-saw-grey-700 " +
    "dark:hover:bg-saw-grey-800 dark:active:bg-saw-grey-900",
  ghost:
    "bg-transparent text-saw-grey-700 hover:bg-saw-grey-100 active:bg-saw-grey-200 " +
    "dark:text-saw-beige dark:hover:bg-saw-grey-800 dark:active:bg-saw-grey-900",
  danger:
    "bg-saw-grey-900 text-saw-white hover:bg-saw-black active:bg-saw-black/90 " +
    "dark:bg-saw-black dark:hover:bg-saw-grey-900",
};

const sizes: Record<Size, string> = {
  sm: "h-8 px-3 text-small",
  md: "h-10 px-4 text-body",
  // PR #55: lg sits at 12 (3rem) tall with body-1 text. The padding
  // step is deliberately wide so the dominant CTA looks like a target
  // rather than just a slightly-taller md button.
  lg: "h-12 px-6 text-body",
};

const Button = forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  { variant = "primary", size = "md", className = "", type = "button", ...rest },
  ref,
) {
  const cls = [base, variants[variant], sizes[size], className]
    .filter(Boolean)
    .join(" ");
  return <button ref={ref} type={type} className={cls} {...rest} />;
});

export default Button;
