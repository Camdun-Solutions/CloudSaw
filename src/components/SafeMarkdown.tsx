// SafeMarkdown — render a strict, allowlisted markdown subset directly into
// React nodes WITHOUT ever using `dangerouslySetInnerHTML`.
//
// Contract 09 §Constraints + Acceptance Criteria: knowledge-base markdown
// MUST be rendered with strict HTML sanitization on an allowlist — no
// scripts, no inline event handlers, no external resource loads. Embedded
// `<script>` (or any raw HTML) MUST render as inert text.
//
// Our approach: we never go to HTML at all. The input string is tokenized
// in pure TS, then each token becomes a regular React element. Any `<` in
// the source is rendered as the text character "<", so a fixture like
// `<script>alert(1)</script>` lands in the DOM as visible text, never as
// an HTML element. That gives a much stronger guarantee than a sanitizer
// that runs after a parse step (no `innerHTML`, no parser ambiguities,
// no need to keep an evolving allowlist in sync with new browser quirks).
//
// Supported tokens:
//   - ATX headings (## …)
//   - Paragraphs (blank-line-separated text)
//   - Fenced code blocks (``` … ```)
//   - Unordered (-, *) and ordered (1.) lists, with shallow nesting
//   - Inline code (`x`), bold (**x**), italic (*x*, _x_), and links.
//     Links are restricted to https://, http:// (https-upgraded by Tauri),
//     and mailto: URLs — javascript:, data:, file: are dropped to text.
//
// Everything else falls through to plain text. There is no MathJax, no
// HTML tag passthrough, no image loading (an image syntax would fetch from
// an external URL, violating CLAUDE.md §5).

import { useMemo, type ReactNode } from "react";

type Props = {
  markdown: string;
  /** Optional class added to the outer wrapper, useful for column widths. */
  className?: string;
  /** Test id forwarded to the wrapper. */
  "data-testid"?: string;
};

// ----- Inline rendering ---------------------------------------------------

const URL_ALLOWLIST = /^(https?:\/\/|mailto:)/i;

/** Render inline markdown (code, bold, italic, links) for a single line. */
function renderInline(text: string, keyPrefix: string): ReactNode[] {
  const out: ReactNode[] = [];
  let i = 0;
  let runStart = 0;
  let runKey = 0;

  const flushRun = (end: number) => {
    if (end > runStart) {
      out.push(text.slice(runStart, end));
      runKey += 1;
    }
  };

  while (i < text.length) {
    const ch = text[i];

    // Inline code: `…`
    if (ch === "`") {
      const end = text.indexOf("`", i + 1);
      if (end !== -1) {
        flushRun(i);
        out.push(
          <code
            key={`${keyPrefix}-c${runKey++}`}
            className="rounded bg-saw-grey-100 dark:bg-saw-grey-800 px-1 py-0.5 font-mono text-small text-saw-grey-900 dark:text-saw-beige"
          >
            {text.slice(i + 1, end)}
          </code>,
        );
        i = end + 1;
        runStart = i;
        continue;
      }
    }

    // Bold: **…**
    if (ch === "*" && text[i + 1] === "*") {
      const end = text.indexOf("**", i + 2);
      if (end !== -1) {
        flushRun(i);
        out.push(
          <strong
            key={`${keyPrefix}-b${runKey++}`}
            className="font-semibold text-saw-grey-900 dark:text-saw-beige"
          >
            {renderInline(text.slice(i + 2, end), `${keyPrefix}-b${runKey}`)}
          </strong>,
        );
        i = end + 2;
        runStart = i;
        continue;
      }
    }

    // Italic: *…* or _…_
    if ((ch === "*" || ch === "_") && text[i + 1] !== ch) {
      const end = text.indexOf(ch, i + 1);
      if (end !== -1 && end !== i + 1) {
        flushRun(i);
        out.push(
          <em
            key={`${keyPrefix}-i${runKey++}`}
            className="italic"
          >
            {renderInline(text.slice(i + 1, end), `${keyPrefix}-i${runKey}`)}
          </em>,
        );
        i = end + 1;
        runStart = i;
        continue;
      }
    }

    // Links: [text](url)
    if (ch === "[") {
      const labelEnd = text.indexOf("]", i + 1);
      if (labelEnd !== -1 && text[labelEnd + 1] === "(") {
        const urlEnd = text.indexOf(")", labelEnd + 2);
        if (urlEnd !== -1) {
          const label = text.slice(i + 1, labelEnd);
          const url = text.slice(labelEnd + 2, urlEnd).trim();
          flushRun(i);
          if (URL_ALLOWLIST.test(url)) {
            out.push(
              <a
                key={`${keyPrefix}-l${runKey++}`}
                href={url}
                rel="noopener noreferrer"
                target="_blank"
                className="text-saw-red underline underline-offset-2 hover:text-saw-red/80"
              >
                {label}
              </a>,
            );
          } else {
            // Disallowed scheme (javascript:, data:, file:, …) — render as
            // inert text so the user can still see what was written.
            out.push(
              <span key={`${keyPrefix}-bl${runKey++}`}>
                {label} ({url})
              </span>,
            );
          }
          i = urlEnd + 1;
          runStart = i;
          continue;
        }
      }
    }

    i += 1;
  }

  flushRun(text.length);
  return out;
}

// ----- Block rendering ----------------------------------------------------

type Block =
  | { kind: "heading"; level: 2 | 3 | 4; text: string }
  | { kind: "paragraph"; lines: string[] }
  | { kind: "code"; text: string; lang: string | null }
  | { kind: "list"; ordered: boolean; items: string[] };

function tokenize(src: string): Block[] {
  const lines = src.replace(/\r\n?/g, "\n").split("\n");
  const blocks: Block[] = [];
  let i = 0;

  while (i < lines.length) {
    const line = lines[i];

    // Skip blank lines between blocks.
    if (line.trim() === "") {
      i += 1;
      continue;
    }

    // Fenced code block.
    const fence = /^```([\w-]*)\s*$/.exec(line);
    if (fence) {
      const lang = fence[1] || null;
      const body: string[] = [];
      i += 1;
      while (i < lines.length && !/^```/.test(lines[i])) {
        body.push(lines[i]);
        i += 1;
      }
      if (i < lines.length) i += 1; // consume closing fence
      blocks.push({ kind: "code", text: body.join("\n"), lang });
      continue;
    }

    // Headings: ## / ### / ####
    const heading = /^(#{2,4})\s+(.+)$/.exec(line);
    if (heading) {
      const level = heading[1].length as 2 | 3 | 4;
      blocks.push({ kind: "heading", level, text: heading[2].trim() });
      i += 1;
      continue;
    }

    // Lists. Unordered: -, *. Ordered: 1. 2. …
    const unordered = /^[-*]\s+(.*)$/.exec(line);
    const ordered = /^\d+\.\s+(.*)$/.exec(line);
    if (unordered || ordered) {
      const isOrdered = !!ordered;
      const items: string[] = [];
      while (i < lines.length) {
        const cur = lines[i];
        const u = /^[-*]\s+(.*)$/.exec(cur);
        const o = /^\d+\.\s+(.*)$/.exec(cur);
        if (isOrdered && o) {
          items.push(o[1]);
          i += 1;
        } else if (!isOrdered && u) {
          items.push(u[1]);
          i += 1;
        } else {
          break;
        }
      }
      blocks.push({ kind: "list", ordered: isOrdered, items });
      continue;
    }

    // Paragraph: consume until blank line or a block-opener.
    const para: string[] = [line];
    i += 1;
    while (i < lines.length) {
      const nxt = lines[i];
      if (
        nxt.trim() === "" ||
        /^```/.test(nxt) ||
        /^#{2,4}\s+/.test(nxt) ||
        /^[-*]\s+/.test(nxt) ||
        /^\d+\.\s+/.test(nxt)
      ) {
        break;
      }
      para.push(nxt);
      i += 1;
    }
    blocks.push({ kind: "paragraph", lines: para });
  }

  return blocks;
}

function renderBlocks(blocks: Block[]): ReactNode[] {
  return blocks.map((b, idx) => {
    switch (b.kind) {
      case "heading": {
        const cls =
          b.level === 2
            ? "mt-6 mb-2 text-h2 font-semibold text-saw-grey-900 dark:text-saw-beige"
            : b.level === 3
              ? "mt-4 mb-2 text-h3 font-semibold text-saw-grey-900 dark:text-saw-beige"
              : "mt-3 mb-1 text-body font-semibold text-saw-grey-900 dark:text-saw-beige";
        const inline = renderInline(b.text, `h${idx}`);
        if (b.level === 2) return <h2 key={`b${idx}`} className={cls}>{inline}</h2>;
        if (b.level === 3) return <h3 key={`b${idx}`} className={cls}>{inline}</h3>;
        return <h4 key={`b${idx}`} className={cls}>{inline}</h4>;
      }
      case "paragraph":
        return (
          <p
            key={`b${idx}`}
            className="my-3 text-body text-saw-grey-800 dark:text-saw-beige whitespace-pre-wrap break-words"
          >
            {renderInline(b.lines.join("\n"), `p${idx}`)}
          </p>
        );
      case "code":
        return (
          <pre
            key={`b${idx}`}
            className="my-3 overflow-x-auto rounded-card bg-saw-grey-900 px-3 py-3 text-small text-saw-grey-50"
          >
            <code
              data-lang={b.lang ?? "text"}
              className="font-mono whitespace-pre"
            >
              {b.text}
            </code>
          </pre>
        );
      case "list": {
        const cls = "my-3 ml-6 space-y-1 text-body text-saw-grey-800 dark:text-saw-beige";
        const items = b.items.map((item, j) => (
          <li key={`b${idx}-i${j}`}>{renderInline(item, `b${idx}i${j}`)}</li>
        ));
        return b.ordered ? (
          <ol key={`b${idx}`} className={`${cls} list-decimal`}>
            {items}
          </ol>
        ) : (
          <ul key={`b${idx}`} className={`${cls} list-disc`}>
            {items}
          </ul>
        );
      }
    }
  });
}

export default function SafeMarkdown({
  markdown,
  className,
  ...rest
}: Props) {
  const blocks = useMemo(() => tokenize(markdown ?? ""), [markdown]);
  const nodes = useMemo(() => renderBlocks(blocks), [blocks]);

  const wrapperCls = ["safe-markdown", className].filter(Boolean).join(" ");
  return (
    <div className={wrapperCls} data-testid={rest["data-testid"]}>
      {nodes}
    </div>
  );
}
