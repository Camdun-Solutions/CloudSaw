import React from "react";
import ReactDOM from "react-dom/client";

import App from "@/App";
import { applyAppearanceImmediate } from "@/lib/appearance";
import { LocaleProvider } from "@/stores/locale";
import { LockProvider } from "@/stores/lock";
import "@/index.css";

// PR #57: apply the persisted appearance preference BEFORE React
// mounts so the user never sees a one-frame flash of light mode
// before the hook flips to dark.
applyAppearanceImmediate();

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <LocaleProvider>
      <LockProvider>
        <App />
      </LockProvider>
    </LocaleProvider>
  </React.StrictMode>,
);
