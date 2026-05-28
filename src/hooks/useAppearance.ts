// useAppearance — PR #57.
//
// Reads + writes the user's appearance preference (light / dark /
// system) and keeps <html class="dark"> in sync. Subscribes to
// `prefers-color-scheme: dark` while in `"system"` mode so the app
// follows the OS theme in real time.
//
// One hook used in two places:
//   1. App.tsx mounts it once at the root so the dark class is kept
//      synced for the whole tree.
//   2. Settings → Appearance reads the current value + calls `setValue`
//      to flip it.

import { useCallback, useEffect, useState } from "react";

import {
  applyAppearance,
  getAppearance,
  type Appearance,
} from "@/lib/appearance";

export function useAppearance(): {
  appearance: Appearance;
  setAppearance: (next: Appearance) => void;
} {
  const [appearance, setAppearanceState] = useState<Appearance>(() =>
    getAppearance(),
  );

  // Apply on mount + whenever the value changes locally.
  useEffect(() => {
    applyAppearance(appearance);
  }, [appearance]);

  // While in "system" mode, follow OS changes live. The listener is
  // detached when the mode flips back to a static choice — no dangling
  // subscription.
  useEffect(() => {
    if (appearance !== "system") return undefined;
    let media: MediaQueryList;
    try {
      media = window.matchMedia("(prefers-color-scheme: dark)");
    } catch {
      return undefined;
    }
    const handler = () => applyAppearance("system");
    media.addEventListener("change", handler);
    return () => media.removeEventListener("change", handler);
  }, [appearance]);

  const setValue = useCallback((next: Appearance) => {
    setAppearanceState(next);
    // Persist + apply (the effect above also applies, but applying
    // here keeps the swap atomic vs. the re-render).
    try {
      localStorage.setItem("cloudsaw.appearance", next);
    } catch {
      // Silent — see lib/appearance.ts for the rationale.
    }
    applyAppearance(next);
  }, []);

  return { appearance, setAppearance: setValue };
}
