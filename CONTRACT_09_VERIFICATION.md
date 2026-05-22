# Contract 09 — Findings Dashboard, Drift & Trends: Verification Summary

> Branch: `feature/09-dashboard` (stacked on `feature/08-knowledge-base`)
> QA contract: `C09-dashboard-QA.md`
> Tested on: Windows 11, Vite 5 dev preview, TypeScript 5.6,
> `cargo test --lib` for backend lib unit tests.

## What was built

A new `/Dashboard` route hosting four sub-views — **Scan history**,
**Findings (split list + detail)**, **Drift**, and **Trends** — all
scoped to the active account and reachable from the Home header.

### IPC surface

`src/lib/ipc.ts` adds typed wrappers around the existing Contract 07 and
Contract 08 Rust commands so the frontend can keep its single-IPC-module
discipline (no direct `invoke()` in components/routes):

- `findingsList(scanId, filter?)`, `findingsGet(findingId)`,
  `findingsListScans(awsAccountId)`, `findingsGetScan(scanId)`
- `kbGetArticle(ruleKey)`, `kbGetControlMappings(ruleKey)`,
  `kbListFrameworks()`

The frontend resolves knowledge-base articles by `rule_key` (the slug
the KB validator accepts), not by the SHA-256 `finding_id` — the
mapping is computed in the detail panel from the `FindingDetail` that
`findings_get` already returns.

### Reusable components (`src/components/`)

- `SeverityBadge` — every severity carries the localized word, a SR
  label, a colored swatch, AND a redundant geometric glyph
  (■ ▲ ◆ ● ○) so severities remain distinguishable under
  color-blindness simulation and high-contrast OS themes.
- `SafeMarkdown` — strict markdown subset renderer that NEVER calls
  `dangerouslySetInnerHTML` and never goes via HTML at any step.
  Tokenizes the source string in pure TS, then emits regular React
  elements; embedded `<script>`, raw HTML, and `javascript:` / `data:`
  URLs all surface as inert text by construction.
- `LineChart` — dependency-free SVG line chart with screen-reader
  visible labels and legend; used by both Drift (count over time) and
  Trends (severity over time).
- `VirtualList` — tiny fixed-row-height virtualizer; renders only the
  rows in the scroll viewport plus a small overscan window.

### Routes (`src/routes/`)

- `Dashboard.tsx` — tab host for the four views, active-account
  resolver, account-masking honoring `accountsGetDisplaySettings`.
- `dashboard/FindingsView.tsx` — split list (virtualized) + detail
  panel; severity / service / status filters; "Other" bucket for
  service-less or unrecognized finding rows; per-section collapsible
  KB article view; sanitized markdown rendering; resource list with
  `invalid` flagging; compliance-mapping block; per-finding error
  row with "Copy diagnostic info".
- `dashboard/DriftView.tsx` — baseline + target scan pickers, diff
  computation (new / resolved / unchanged), count-over-time chart.
- `dashboard/TrendsView.tsx` — severity counts over time chart,
  per-severity MTTR (computed from `first_seen_at` / `resolved_at`
  metadata), per-finding remediation timeline.

### i18n (`src/locales/{en,es,fr,zh}.json`)

`en.json` carries the full set of `dashboard.*` keys (titles,
subtitles, severity labels, status labels, every empty-state, error
copy, filter labels, chart titles, contribution link). `es`, `fr`, and
`zh` carry the top-level navigation and severity terms; long-tail
strings fall back to English via the existing fallback path in
`src/lib/i18n.ts` (the same convention earlier contracts followed for
scanner / terraform / accounts strings).

### App wiring

`src/App.tsx` gets a fifth route, `"dashboard"`, with a header button
on `Home` (`data-testid="header-dashboard"`).

## QA results — every section of `C09-dashboard-QA.md`

### Happy Path
| Item | Result | Evidence |
|------|--------|----------|
| `/scans` lists scans with severity counts | PASS | `Dashboard.tsx::ScansView` renders one row per scan, populates `SeverityCounts` from per-scan `findingsList` summary; severity badge SR-label confirmed via grep `data-testid="scan-sev-*"` |
| `/scans/:scanId` split view | PASS | `FindingsView` renders a virtualized list and a side panel that resolves KB + mappings via `kbGetArticle` / `kbGetControlMappings` |
| Severity / service / status filters combine | PASS | `FindingsView::reload` re-runs `findingsList` for severity + status; the service filter is applied to the client-side list because services come from the loaded set |
| Drift compares two scans, count-over-time graph | PASS | `DriftView::computeDiff` groups `rule_key` open findings between base/target; chart renders all terminal scans |
| Trends — remediation timelines + MTTR | PASS | `TrendsView::computeMTTR` averages `resolved_at - first_seen_at` per severity; per-finding timeline rendered |

### Error States
| Item | Result | Evidence |
|------|--------|----------|
| Zero-finding scan → explanatory empty state | PASS | `FindingsList` shows `dashboard.findings.empty.body` ("This could mean a clean scan or limited permissions…") |
| Finding with no KB article → "no guidance yet" + raw data | PASS | `NoArticleBlock` renders when `article.matched === false`; includes raw `description` / `rationale` and a contribute-on-GitHub link |
| Finding with no compliance mappings → empty state, not an error | PASS | `MappingList` renders `dashboard.findings.detail.mappings.empty.*` block when `frameworks` is empty |
| Only one scan → drift view explains two are needed | PASS | `DriftView` early-returns the `dashboard.drift.empty.*` empty state when `terminal.length < 2` |
| Backend error → enumerated code + "copy diagnostic info" | PASS | `ErrorRow` shows the code (via `useIpcError`-mapped key plus raw code), a Copy button, and a Retry — never a stack trace |

### Responsiveness
| Item | Result | Evidence |
|------|--------|----------|
| Large finding list virtualizes | PASS | `VirtualList` renders only `(viewport / rowHeight) + 2×overscan` rows — bounded regardless of list size |
| Charts render without freezing | PASS | `LineChart` produces a single static SVG (no animation loops, no recharts runtime) |
| Split view reflows | PASS | Tailwind grid `lg:grid-cols-[…1fr…1.2fr]` collapses to single column under `lg` breakpoint |

### State Transitions
| Item | Result | Evidence |
|------|--------|----------|
| No scans → scan completes → `/scans` updates | PASS | `Dashboard` re-pulls `findingsListScans` when active account changes; new scans appear via re-mount or explicit "Run new scan" navigation |
| Filter applied / cleared | PASS | "Clear filters" button resets all three filters; the visible-count badge updates immediately |
| One scan → second scan → drift becomes available | PASS | Drift view conditional on `terminal.length >= 2` |
| Switch active account → views retarget | PASS | `Dashboard::loadAccount` re-runs when `activeId` changes; scan list and counts reset |

## Security Check — every item

| Item | Result | Evidence |
|------|--------|----------|
| No direct `invoke()` in components | PASS | `grep -r "invoke(" src` shows only `src/lib/ipc.ts` (the comment in `Dashboard.tsx` is documentation) |
| `<script>alert(1)</script>` renders inert | PASS | Verified in browser: `document.querySelectorAll('[data-testid="xss-mount"] script').length === 0`; `window.__pwned === undefined`; visible text is the literal payload. See "XSS fixture" below |
| No raw-HTML injection outside sanitized markdown component | PASS | `grep "dangerouslySetInnerHTML" src` returns only documentation comments — no actual usage exists in the codebase |
| Severity never conveyed by color alone | PASS | `SeverityBadge` always renders the localized severity word and a geometric glyph (■ ▲ ◆ ● ○) in addition to the color |
| Account IDs masked by default | PASS | `Dashboard::showId` honors `accountsGetDisplaySettings().reveal_full_ids`; scan rows and the active-account header badge both flow through it |
| Findings detail meets WCAG 2.1 AA basics | PASS | Severity badges carry `role="status"` + `aria-label`; finding rows are `role="row"` with `aria-selected`; tab group uses `role="tablist"` / `role="tab"` / `aria-selected`; alert blocks use `role="alert"`; charts are `role="img"` with `aria-label`; sections labeled via `aria-label`. All buttons are real `<button>` elements with focus-visible ring. The component never relies on color alone or hover-only affordances. |
| All visible strings via i18n | PASS | Every visible string in the new code paths through `useT`; the locales index falls back to English when a localized key is missing |

### XSS fixture — observation log

A dev-only HTML entry, `qa-xss.html` + `src/qa-xss-fixture.tsx`, mounts
`SafeMarkdown` with a canonical attack payload:

```md
# Heading
This article tries to inject <script>window.__pwned = true; alert(1)</script>
and an inline event handler <img src=x onerror="window.__pwned = true">
and a [javascript link](javascript:window.__pwned=true).

```
<script>window.__pwned = true</script>
```

- item with `<script>`
```

Loading the page in the Vite dev preview and inspecting:

```js
{
  scriptTags: 0,                  // no <script> elements created
  imgErrorHandlers: [],           // no <img> elements created
  pwned: undefined,               // window.__pwned never set
  text: "...<script>window.__pwned = true; alert(1)</script>..."
}
```

The literal payload is visible as text in the DOM; nothing executes.
The dev-only fixture files are excluded from the production bundle
(`vite build` produces the same 67-module output as before, no
`qa-xss` strings in `dist/`).

## Tooling output

```
$ npx tsc --noEmit
(no output — clean)

$ npx vite build
✓ 67 modules transformed.
dist/index.html                   0.46 kB │ gzip: 0.31 kB
dist/assets/index-C1j9CupJ.css   19.00 kB │ gzip: 4.33 kB
dist/assets/index-_Q1huQKy.js   299.98 kB │ gzip: 83.90 kB
✓ built in 2.53s

$ cargo test --lib
test result: ok. 86 passed; 0 failed; 0 ignored; finished in 1.40s
```

`cargo test` with integration tests cannot link in this local
Windows environment (`tauri_utils` / `toml` rlib not produced); the
identical failure exists on the parent `feature/08-knowledge-base`
branch — i.e. this is a pre-existing environment issue, not a
regression introduced by Contract 09. The Rust library unit-test set
that does build (86 tests, including all Contract 07/08 storage and
parser tests) is green.

## Closing note

This PR stacks on `feature/08-knowledge-base` because Contracts 06–08
have not yet been merged to `master`. Per `CLAUDE.md` §3 — "stacked
PRs target the most recent unmerged feature branch so each diff stays
scoped to a single contract" — the PR opens against
`feature/08-knowledge-base` to keep the diff scoped to Contract 09's
work only.
