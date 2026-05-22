// QA fixture page (dev-only). Renders SafeMarkdown with a canonical XSS
// payload so the QA contract's "no script executes" check can be verified
// in a browser without spinning up a full Tauri host.
//
// This file is NOT imported from the production bundle (main.tsx); it
// exists only at the dev-time HTML entry `qa-xss.html`.

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import SafeMarkdown from "@/components/SafeMarkdown";
import "./index.css";

const XSS_PAYLOAD = `# Heading

This article tries to inject <script>window.__pwned = true; alert(1)</script>
and an inline event handler <img src=x onerror="window.__pwned = true">
and a [javascript link](javascript:window.__pwned=true).

\`\`\`
<script>window.__pwned = true</script>
\`\`\`

- item with \`<script>\`
`;

const root = createRoot(document.getElementById("root") as HTMLElement);
root.render(
  <StrictMode>
    <div style={{ padding: 16, fontFamily: "system-ui" }}>
      <h1>SafeMarkdown XSS fixture</h1>
      <p data-testid="xss-status">
        Inspect <code>window.__pwned</code>. It must remain <code>undefined</code>.
      </p>
      <SafeMarkdown markdown={XSS_PAYLOAD} data-testid="xss-mount" />
    </div>
  </StrictMode>,
);
