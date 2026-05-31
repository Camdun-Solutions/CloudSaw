# CloudSaw manual QA checklist — 2026-05-28

This file is the working surface for the QA-buddy walkthrough that
covers what the Vite browser preview couldn't (Tauri runtime, AWS,
SQLite, keychain, file system, OS notifications).

The companion [`QA_REPORT_2026-05-28.md`](QA_REPORT_2026-05-28.md)
documents what was already verified statically + via the browser
preview. PR #59 fixed 4 of the 5 findings from that report; FINDING-001
(cargo test integration build) is deferred to a workspace-split PR.

## How to use this file

Walk each section against the real Tauri shell on your Windows
machine. For each item, edit the line in place:

- `- [x] <item>` → passed cleanly. No additional note needed.
- `- [ ] <item> — FAIL: <one-liner>` → failed. Add the one-liner
  inline so future-Claude can find it. Example:
  `- [ ] Lock now button → FAIL: button greys out but session
  doesn't actually lock; have to click twice`
- Leave `- [ ] <item>` (no annotation) for items you haven't gotten
  to yet.
- `- [SKIP] <item>` if the test isn't applicable to your environment
  (e.g., macOS-only items on Windows).

When you're ready to debrief, hand the file back via a new session
referencing
[`~/.claude/plans/session-handoff-2026-05-28-qa-buddy.md`](~/.claude/plans/session-handoff-2026-05-28-qa-buddy.md).
Claude will read this file, pull the `FAIL:` lines, and start
cutting follow-up PRs.

## Optional — high-leverage data captures (do these first if time allows)

These don't require walking the full list. Pick the ones that fit
your time budget; even just (1) meaningfully expands what Claude can
verify between sessions.

- [ ] **(1) IPC fixture capture** — In Tauri DevTools (F12) console:
  ```js
  const real = window.__TAURI_INTERNALS__.invoke;
  window.__capturedIPC__ = {};
  window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
    const result = await real(cmd, args);
    if (!(cmd in window.__capturedIPC__)) {
      window.__capturedIPC__[cmd] = { sampleArgs: args, sampleResult: result };
    }
    return result;
  };
  ```
  Walk a few flows (Dashboard, Findings, every Settings section), then:
  ```js
  copy(JSON.stringify(window.__capturedIPC__, null, 2));
  ```
  Paste the result into the bottom of this file under a new
  `## Captured IPC fixtures` heading. Redact account labels / IDs
  freely.

- [ ] **(2) Screenshots** of every route in light + dark mode after
  running a real scan. Drop them into a `qa-screenshots/` directory
  and reference them inline by path. Suggested set:
  - [ ] Home (with recent activity + top findings)
  - [ ] Findings (per-service expanded, severity borders, AI inline preview if a key is connected)
  - [ ] Settings → App lock
  - [ ] Settings → Accounts
  - [ ] Settings → Appearance
  - [ ] Settings → Notifications
  - [ ] Settings → Schedules
  - [ ] Settings → Activity log
  - [ ] Settings → Onboarding
  - [ ] Settings → Reports
  - [ ] Settings → Retention
  - [ ] Settings → Updates
  - [ ] Settings → GitHub
  - [ ] Settings → AI
  - [ ] Settings → Panic
  - [ ] Exported HTML report opened in a browser

- [ ] **(3) Tauri shell log capture** for one full session:
  ```powershell
  $env:RUST_LOG = "debug"
  npm run tauri dev *> tauri-dev.log
  ```
  Then paste a `tail -200` of `tauri-dev.log` into the bottom of this
  file under `## Tauri shell log tail` — or just attach the whole
  file to the PR.

---

## Phase 1 — Lock + identity

- [ ] First-run wizard: set master password (≥8 chars), complete all 6 steps from a fresh DB
- [ ] Lock now → unlock with password
- [ ] Re-lock period selector works end-to-end
  - [ ] "Immediate" — session locks on app focus loss
  - [ ] "1 day" — session persists for ~24h before re-lock
- [ ] Biometric unlock (Windows Hello), if your device has it set up
- [ ] Recovery flow — forgotten password → OS identity verification → new password
- [ ] Recovery rejects when no biometric / PIN is configured at the OS level (graceful unavailable message)
- [ ] App locks automatically after the configured re-lock period elapses while open

## Phase 2 — Onboarding wizard (Contract 14)

- [ ] Language step persistence across full app restart (close & reopen, language sticks)
- [ ] Password step rejects passwords shorter than the configured min length
- [ ] Account step verifies against the `cloudsaw` AWS profile (sts:GetCallerIdentity round-trip)
- [ ] Account step rejects when profile resolves to an account-ID that's already configured
- [ ] Terraform step provisions the scanner role end-to-end against the verified account
- [ ] Context step writes business context to SQLite (verify by reopening Settings → AI after finishing)
- [ ] First-scan step (PR #52): "Scan now" → terminal state → auto-navigate to Findings
- [ ] Kill app mid-wizard → relaunch resumes at the same step
- [ ] "I'll do this myself" skip path lands at Dashboard

## Phase 3 — Accounts + Profiles

- [ ] Add account (verify against real profile, idempotent label rejection on duplicate)
- [ ] Edit account, change profile mapping (rejects if new profile resolves to a different AWS account ID)
- [ ] Remove account — impact preview accurate (scans count, findings count, tf workdir present)
- [ ] Remove account — deletion is immediate, no undo, removed row not in `accounts_list` after refresh
- [ ] Profile test — success path
- [ ] Profile test — `sso_expired` failure path (revoke SSO session, retry)
- [ ] Profile test — `permission_denied` failure path
- [ ] Profile test — `connectivity` failure path (disable network briefly)
- [ ] Profile test — `timeout` failure path
- [ ] Active-account switch reflected on Dashboard + Findings without refresh
- [ ] Reveal-full-IDs toggle in Accounts shows full 12 digits; off shows mask `****1234`

## Phase 4 — Scanner role provisioning (Contract 05)

- [ ] Terraform plan generates against AWS (look for `terraform.provision.detect.available` in the modal)
- [ ] Plan diff renders correctly in the modal
  - [ ] Create rows render
  - [ ] Update rows render
  - [ ] Delete rows render
  - [ ] No-op rows render
- [ ] SecurityAudit (recommended) policy variant applies cleanly
- [ ] ReadOnlyAccess policy variant applies cleanly (with the broader-permissions warning shown)
- [ ] Apply succeeds with trust-policy SHA verified (success banner + role ARN displayed)
- [ ] Re-plan after deleting the role outside CloudSaw → detects drift, plan shows Create again
- [ ] Re-plan after a manual policy change → detects drift, plan shows Update

## Phase 5 — Scans + Findings (the core flow)

- [ ] End-to-end scan against `cloudsaw` AWS profile completes successfully
- [ ] ScoutSuite binary SHA verified before each scan (check log line)
- [ ] AssumeRole succeeds + session expires after scan
- [ ] Findings written to SQLite (check `findings` table row count > 0)
- [ ] `raw-scout.json` written to per-scan directory
- [ ] Findings page (PR #51): per-service collapsible groups render
- [ ] Findings page: severity-colored left borders per card
  - [ ] critical = black
  - [ ] high = red
  - [ ] medium = orange
  - [ ] low = gold
  - [ ] info = grey
  - [ ] resolved = green
- [ ] Findings page: scan filter dropdown lists recent scans
- [ ] Findings page: search filter matches multiple fields
  - [ ] matches `dashboard_name`
  - [ ] matches `rule_key`
  - [ ] matches `description`
- [ ] Findings page: clear filters resets all four filter controls
- [ ] AI inline preview (PR #58): with provider key configured, click "AI suggestion" → preview unfurls below the CTA (NOT a modal)
- [ ] AI inline preview shows all expected fields
  - [ ] provider
  - [ ] model
  - [ ] digest
  - [ ] business context
  - [ ] identifying-field flags
  - [ ] placeholder list
  - [ ] system prompt
  - [ ] user message
- [ ] AI Cancel at preview → zero outbound HTTPS to the provider host (verify via `netstat -an | findstr :443` or DevTools Network tab)
- [ ] AI Send → loading spinner, then suggestion appears with "AI-generated, unreviewed" pill + token usage line
- [ ] AI suggestion error path: invalidate key in provider UI mid-session → error message renders, KB article above still usable
- [ ] Remediation variant tabs (PR #58): on a finding with KB article that carries all three variants, the Remediation disclosure shows the tab strip
  - [ ] Overview tab renders and shows the overview body
  - [ ] Terraform Fix tab renders and swaps the markdown body
  - [ ] AWS CLI Fix tab renders and swaps the markdown body
- [ ] Remediation tabs: on a finding with only an Overview, the tab strip shows just Overview (single tab, no empty tabs)
- [ ] Drift view (compare two scans) — buckets populate
  - [ ] "new" bucket lists findings introduced in the later scan
  - [ ] "resolved" bucket lists findings only present in the earlier scan
  - [ ] "unchanged" bucket lists findings present in both
- [ ] Drift view: same scan picked twice → "Pick two different scans" empty state
- [ ] Trends view — MTTR + per-finding remediation timeline render with real data

## Phase 6 — Knowledge base + GitHub

- [ ] KB remote refresh flow
  - [ ] opt-in toggle saves
  - [ ] check for update finds the remote bundle
  - [ ] apply succeeds and switches articles to the remote bundle
  - [ ] revert returns to bundled articles
- [ ] KB remote refresh: bundled article still available as offline fallback after remote is active
- [ ] GitHub token save via PAT — token stored only in OS keychain (verify via Credential Manager)
- [ ] GitHub findings-repo set, then create a finding ticket via API path
- [ ] Finding ticket linked-row replaces the "Create ticket" CTA after success
- [ ] GitHub browser-fallback path (no token configured) opens browser with prefilled new-issue URL
- [ ] Error-report submission: paste an ARN in the notes field → ARN is truncated in the redaction preview
- [ ] Error-report submission: paste an AWS account ID → masked to `****1234` in the redaction preview
- [ ] Error-report submission: token-bearing patterns redacted
  - [ ] GitHub PATs (`ghp_…`) redacted
  - [ ] OpenAI-style keys (`sk-…`) redacted

## Phase 7 — Schedules + Activity log + Retention

- [ ] Configure a "Daily" schedule for an account; verify next-run timestamp displayed
- [ ] Configure a "Weekly" schedule with a day-of-week selector
- [ ] Configure an "Every N minutes" interval schedule
- [ ] Disable a schedule → config preserved, next-run line says "Disabled"
- [ ] Catch-up scan after missed slot: close app, advance system clock past the next scheduled time, relaunch → catch-up scan fires
- [ ] Activity log records scan completions with severity counts
- [ ] Activity log records account lifecycle events
  - [ ] account add
  - [ ] account remove
  - [ ] set-active
- [ ] Activity log records password change events (no password material in the row)
- [ ] Activity log records system events
  - [ ] export events (HTML / PDF / custom)
  - [ ] retention purge events
  - [ ] panic wipe events
- [ ] Activity log search filters by free-text against summary + detail
- [ ] Activity log filter dropdown narrows to a single event kind
- [ ] Activity log "Clear view" hides rows but underlying SQLite rows persist (verify by re-opening the page)
- [ ] Activity log export
  - [ ] copy to clipboard
  - [ ] save to file
- [ ] Retention purge — manual run on demand
- [ ] Retention purge — each scan period saves
  - [ ] 30 days
  - [ ] 60 days
  - [ ] 90 days
  - [ ] 180 days
  - [ ] 365 days
  - [ ] never
- [ ] Retention purge — each event-log period saves independently of scan period
- [ ] After a retention purge: scan rows older than the threshold removed; findings metadata untouched

## Phase 8 — Reports

- [ ] Per-scan HTML export: file writes successfully
- [ ] Per-scan HTML export (PR #56): opens in a browser; per-service `<details>` groups render
- [ ] HTML export: severity-colored left borders on finding cards
- [ ] HTML export: remediation tabs not present (HTML export uses stacked disclosures, not tabs — PR #56)
- [ ] HTML export: review-banner present at top
- [ ] HTML export: brand logo visible in header
- [ ] HTML export: no `<script>` tags anywhere (view-source check)
- [ ] HTML export: no remote URLs (search for `http://` or `https://` in view-source)
- [ ] HTML export: masked account IDs by default; full account IDs only on explicit opt-in
- [ ] Per-scan PDF export: file writes successfully
- [ ] PDF export: opens in default PDF reader; pagination works
- [ ] PDF export: no Helvetica glyph dropouts on Latin-1 content
- [ ] PDF export: mandatory review banner at top of page 1
- [ ] PDF export: account ID disclosure mode shown in footer
- [ ] Custom report builder: account selector + date range picker + preview pane work
- [ ] Custom report builder: export honors both filters in the resulting file
- [ ] Auto-export folder writes a copy on every scan (verify by checking the folder after a scan)
- [ ] Auto-export failure (e.g., folder no longer exists): primary export still succeeds; UI shows auto-export-failed row

## Phase 9 — Destructive paths

- [ ] Delete scan (hard delete): confirm typing matches required token (DELETE or scan ID)
- [ ] Delete scan: SQLite row + per-scan directory + raw file all removed; VACUUM run; toast message accurate
- [ ] Delete scan with secure-overwrite checkbox: completes without error
- [ ] Panic wipe input validation (fix from QA verified)
  - [ ] exact "PANIC" → submit button enabled
  - [ ] padded " PANIC " → submit button stays disabled (no trim)
  - [ ] lowercase "panic" → submit button stays disabled (case-sensitive)
  - [ ] XSS payload (e.g. `<script>alert(1)</script>`) → submit button stays disabled
- [ ] Panic wipe: SQLite DB file removed
- [ ] Panic wipe: every CloudSaw keychain entry cleared (verify via Windows Credential Manager — search "cloudsaw")
- [ ] Panic wipe: scan dirs, terraform workdirs, log files all removed
- [ ] Panic wipe: self-delete helper staged for next boot (success message says "is staged to run on the next boot")
- [ ] Panic wipe: reboot prompt offered

## Phase 10 — Auto-updater + Notifications

- [ ] Auto-updater banner appears when feed advertises a newer version (use a test feed if available)
- [ ] Updater verifies Ed25519 signature against the configured public key before applying
- [ ] "Check for updates" button respects the auto-check toggle pref
- [ ] Desktop notification on scan complete (PR #54): with toggle enabled in Settings → Notifications, run a scan → Windows toast appears
- [ ] Desktop notification: toggle disabled → no toast on scan completion
- [ ] Desktop notification: first send triggers OS permission prompt; deny → subsequent sends silently no-op
- [ ] Desktop notification: title + body match the locale strings (verify by switching language)

## Phase 11 — PR #59 visual verification (cross-check the fixes I just shipped)

- [ ] **FINDING-002 fix**: flip Settings → Appearance → Dark; TopNav top-right reads as a dark surface (saw-grey-dark), not white
- [ ] **FINDING-004 fix**: open Change Password modal → page underneath cannot be scrolled by mouse wheel / touchpad
- [ ] **FINDING-004 fix**: open Change Password → open Export Report (or any second modal); close the second → body scroll STILL locked because first is still open
- [ ] **FINDING-004 fix**: close all modals → body scroll restored
- [ ] **FINDING-005 fix**: open Change Password with 1Password / Bitwarden installed → password manager offers "fill current password" + "suggest new password" (autocomplete + minLength engaged)
- [ ] **FINDING-003 fix**: any e2e tests that referenced `settings-section-activitylog` or `settings-section-reports` should now use `settings-section-activity_log` and `settings-section-report` — no broken tests

## Phase 12 — Cross-environment

- [ ] **macOS scan path** — scoutsuite_results_cloudsaw.js filename + PyInstaller `data-files` bundling (regression check on PR #36's fix). Run a scan on macOS, verify findings show in dashboard
- [ ] **macOS biometric unlock** (Touch ID)
- [ ] High-DPI Windows at 200% scale
  - [ ] logo renders crisply (not pixelated, not blurry)
  - [ ] finding cards render without truncation
  - [ ] tab strips remain legible
- [ ] Window resize from 800×600 minimum (PR #38) up to ultra-wide
  - [ ] TopNav remains visible at the minimum size
  - [ ] Logo doesn't clip at the minimum size
  - [ ] Findings table layout adapts across the range
- [ ] First boot on a clean Windows machine without AWS CLI installed: account add flow surfaces the helpful error pointing to `aws configure`

## Phase 13 — Edge cases worth deliberate exercise

- [ ] Lock screen during an in-flight scan: scan continues to terminal state; UI re-locks on unlock with scan results intact
- [ ] Long resource list — scan an account with 1000+ flagged resources: per-finding 50-item cap enforced + "+N more" message renders
- [ ] Locale switch mid-session in Settings: all routes re-render with the new locale; no stale strings
- [ ] AWS SSO session expires mid-scan: `aws.error.sso_expired` path engages; scan record marked failed with `scanner_assume_role_failed`
- [ ] Paste a multi-line PAT into GitHub token field: rejected or trimmed (verify the saved token is single-line)
- [ ] Paste a multi-line API key into AI key field: rejected or trimmed
- [ ] Filesystem permission denied on data directory: bug-report fallback in App.tsx triggers with actionable copy
- [ ] Malformed `raw-scout.json` (manually corrupt the file post-scan): parser doesn't crash; finding count = 0; `output_missing` or `parse_error` code surfaces in UI

---

## When this is done

Open a new session and reference
[`~/.claude/plans/session-handoff-2026-05-28-qa-buddy.md`](~/.claude/plans/session-handoff-2026-05-28-qa-buddy.md).
Claude will:

1. Pull this file from the merged PR.
2. Scan for `FAIL:` lines.
3. Categorize them (visual / functional / blocker).
4. Cut follow-up PRs for the fixable items.
5. Update [`QA_REPORT_2026-05-28.md`](QA_REPORT_2026-05-28.md) with the
   final pass/fail tallies.
