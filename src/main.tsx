import React from "react";
import ReactDOM from "react-dom/client";

import App from "@/App";
import { LocaleProvider } from "@/stores/locale";
import { LockProvider } from "@/stores/lock";
import "@/index.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <LocaleProvider>
      <LockProvider>
        <App />
      </LockProvider>
    </LocaleProvider>
  </React.StrictMode>,
);
