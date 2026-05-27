// CloudSaw branded logo — single source of truth for every header,
// avatar, splash, and chrome surface in the app.
//
// The underlying asset is `src/assets/cloudsaw-logo.png` (1024x1024,
// padded square with transparent background — generated from the
// source artwork by the one-shot prep script that ran during the
// `feature/logo-replacement` PR; the source PNG is the same one
// `src-tauri/icons/source.png` reads, so Tauri's window icon and the
// React in-app logo are always in lockstep).
//
// Sizes intentionally constrained to a small enum rather than free
// pixel values so designers can audit every render site at a glance
// — if you find yourself needing a custom size, please add a new
// preset here with a brief justification rather than passing pixel
// counts inline.

import logoUrl from "@/assets/cloudsaw-logo.png";

type LogoSize = "xs" | "sm" | "md" | "lg" | "xl";

const SIZE_CLASSES: Record<LogoSize, string> = {
  // HEIGHT-based sizing with auto width so the cloud renders at
  // its natural 2.1:1 aspect ratio. PR #37 + #40 used `h-X w-X`
  // (square box) but the underlying asset was a horizontal cloud
  // padded onto a square canvas — the cloud ended up ~50% the
  // visible size of the box. PR #44 changes the asset itself to
  // a bounding-box-cropped non-square PNG; this rule lets that
  // natural aspect flow through.
  xs: "h-7 w-auto", // top-of-list avatars, compact rows
  sm: "h-10 w-auto", // page-header chrome (Home / Dashboard /
  //                    Accounts / Profiles / UnlockScreen /
  //                    FirstRunSetup — reads cleanly next to a
  //                    text-h2 title)
  md: "h-14 w-auto", // onboarding step header
  lg: "h-20 w-auto", // splash, empty-state hero
  xl: "h-28 w-auto", // unlock screen, first-run welcome
};

type Props = {
  size?: LogoSize;
  className?: string;
  /** Forwarded to the underlying <img>. Decorative use should pass
   *  `aria-hidden="true"`; informational use should pass a real alt. */
  alt?: string;
};

export function Logo({ size = "sm", className = "", alt = "" }: Props) {
  return (
    <img
      src={logoUrl}
      alt={alt}
      aria-hidden={alt === "" ? true : undefined}
      className={`${SIZE_CLASSES[size]} ${className}`.trim()}
      // PNG is pre-padded with transparent background — no rounded
      // corner needed at the component level. Surfaces that want a
      // rounded badge can pass `className="rounded-card"`.
    />
  );
}

export default Logo;
