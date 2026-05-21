import { forwardRef, type ButtonHTMLAttributes } from "react";

type Variant = "primary" | "secondary" | "ghost" | "danger";

type Size = "sm" | "md";

type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: Variant;
  size?: Size;
};

const base =
  "inline-flex items-center justify-center gap-2 rounded-card font-medium " +
  "transition-colors disabled:cursor-not-allowed disabled:opacity-60 " +
  "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-saw-orange " +
  "focus-visible:ring-offset-2 focus-visible:ring-offset-saw-grey-50";

const variants: Record<Variant, string> = {
  primary:
    "bg-saw-red text-saw-white hover:bg-saw-red/90 active:bg-saw-red/80",
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
