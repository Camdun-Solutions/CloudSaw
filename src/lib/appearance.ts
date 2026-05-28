// Appearance (Light / Dark / Match system) — PR #57.
//
// The user's preference is persisted to localStorage (key:
// `cloudsaw.appearance`). Same pattern PR #54 uses for the
// scan-notification opt-in — a user-facing UI pref that doesn't need
// cross-device sync, doesn't earn a SQLite migration.
//
// Default: `"system"`. The hook (useAppearance) watches
// `prefers-color-scheme: dark` while the setting is `"system"` so the
// app follows the OS in real time.
//
// Synchronous read so the initial render can apply the right class
// without a paint flash — `applyAppearanceImmediate` is called from
// `main.tsx` before React mounts.

export type Appearance = "light" | "dark" | "system";

const STORAGE_KEY = "cloudsaw.appearance";

const VALID: ReadonlyArray<Appearance> = ["light", "dark", "system"];

function isAppearance(v: string | null): v is Appearance {
  return v !== null && (VALID as ReadonlyArray<string>).includes(v);
}

export function getAppearance(): Appearance {
  try {
    const v = localStorage.getItem(STORAGE_KEY);
    return isAppearance(v) ? v : "system";
  } catch {
    return "system";
  }
}

export function setAppearance(value: Appearance): void {
  try {
    localStorage.setItem(STORAGE_KEY, value);
  } catch {
    // localStorage write errors are exotic; silent — useAppearance
    // re-reads on every render so a transient failure just means the
    // next page-load reverts to the prior value.
  }
  applyAppearance(value);
}

/** Resolve the effective mode for a given setting. `"system"` consults
 *  `prefers-color-scheme: dark`; the two explicit modes pass through. */
export function resolveEffective(value: Appearance): "light" | "dark" {
  if (value === "light") return "light";
  if (value === "dark") return "dark";
  try {
    return window.matchMedia("(prefers-color-scheme: dark)").matches
      ? "dark"
      : "light";
  } catch {
    // No matchMedia (server-side render, exotic embed) — assume light.
    return "light";
  }
}

/** Set / clear the `dark` class on <html> for the resolved mode. Safe
 *  to call repeatedly; idempotent. */
export function applyAppearance(value: Appearance): void {
  try {
    const effective = resolveEffective(value);
    document.documentElement.classList.toggle("dark", effective === "dark");
  } catch {
    // No document (non-browser) — silent. The hook will retry on mount.
  }
}

/** Apply the persisted preference immediately, before React mounts.
 *  Avoids the brief paint flash that would otherwise show in light
 *  mode for a frame before the hook flips to dark. */
export function applyAppearanceImmediate(): void {
  applyAppearance(getAppearance());
}
