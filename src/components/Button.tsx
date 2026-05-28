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

const base =
  "inline-flex items-center justify-center gap-2 rounded-card font-semibold " +
  "transition-colors disabled:cursor-not-allowed disabled:opacity-60 " +
  "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-saw-orange " +
  "focus-visible:ring-offset-2 focus-visible:ring-offset-saw-grey-50";

const variants: Record<Variant, string> = {
  // PR #55: primary CTAs use the bolder saw-red-bold token so the
  // dominant action reads as the strongest element on the page.
  // Hover/active states stay on the standard saw-red ramp.
  primary:
    "bg-saw-red-bold text-saw-white hover:bg-saw-red active:bg-saw-red/90",
  secondary:
    "bg-saw-white text-saw-grey-900 border border-saw-grey-300 " +
    "hover:bg-saw-grey-100 active:bg-saw-grey-200",
  ghost:
    "bg-transparent text-saw-grey-700 hover:bg-saw-grey-100 active:bg-saw-grey-200",
  danger:
    "bg-saw-grey-900 text-saw-white hover:bg-saw-black active:bg-saw-black/90",
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
