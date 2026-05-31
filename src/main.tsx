import React from "react";
import ReactDOM from "react-dom/client";
import { openUrl } from "@tauri-apps/plugin-opener";

import App from "@/App";
import { applyAppearanceImmediate } from "@/lib/appearance";
import { LocaleProvider } from "@/stores/locale";
import { LockProvider } from "@/stores/lock";
import "@/index.css";

// PR #57: apply the persisted appearance preference BEFORE React
// mounts so the user never sees a one-frame flash of light mode
// before the hook flips to dark.
applyAppearanceImmediate();

// PR #84 — Global external-link click delegate. Tauri's webview
// silently drops `target="_blank"` clicks (the navigation is
// intercepted but no external browser is launched), so every doc
// reference / GitHub link / scoutsuite reference that's rendered as
// a plain `<a target="_blank">` becomes inert. This delegate routes
// such clicks through `@tauri-apps/plugin-opener` instead, which
// opens the URL in the user's OS default browser as a new tab in
// their existing window.
//
// Scope: any `<a>` whose href starts with `http://`, `https://`, or
// `mailto:`. Internal `#anchor` or relative-path navigations fall
// through untouched. Modifier-key clicks (Ctrl/Cmd/Shift, middle
// mouse) also fall through so power users keep their browser
// shortcuts working.
document.addEventListener("click", (e) => {
  // Bail on modifier-key / middle-mouse so users can still
  // "open in new private window" etc. via their OS-level shortcuts.
  const mouseEvent = e as MouseEvent;
  if (
    mouseEvent.metaKey ||
    mouseEvent.ctrlKey ||
    mouseEvent.shiftKey ||
    mouseEvent.altKey ||
    mouseEvent.button === 1
  ) {
    return;
  }
  const target = e.target;
  if (!(target instanceof Element)) return;
  const anchor = target.closest("a");
  if (!anchor) return;
  const href = anchor.getAttribute("href");
  if (!href) return;
  // Route only external schemes — let the SPA router handle
  // hash / relative links on its own.
  if (!/^(https?:\/\/|mailto:)/i.test(href)) return;
  e.preventDefault();
  void openUrl(href);
});

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <LocaleProvider>
      <LockProvider>
        <App />
      </LockProvider>
    </LocaleProvider>
  </React.StrictMode>,
);
