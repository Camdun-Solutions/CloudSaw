# Contract 12 Verification — GitHub Integration

Contract: `cloud-saw-contracts/C12-github-integration.md`
QA contract: `cloud-saw-contracts/C12-github-integration-QA.md`
Branch: `feature/12-github-integration`
Verifier: automated test suite (`src-tauri/tests/qa12_test.rs`) plus the
operator-driven checks called out below.

Contract 12 delivers two related capabilities sharing a single fine-
grained PAT — **12A** error reporting (redacted bundle + direct API
submission + browser fallback) and **12B** per-finding GitHub Issue
tickets — plus a Settings page for PAT and repo configuration.

---

## What landed in this contract

**Rust modules**

| Module | Purpose |
|---|---|
| [`github/`](src-tauri/src/github/mod.rs) | Public surface: settings, prepare/submit error reports, prepare/submit finding tickets, browser-fallback URL builder. |
| [`github/pat.rs`](src-tauri/src/github/pat.rs) | Keychain-backed PAT storage. Reads/writes ONLY through `keychain::{GITHUB_PAT_SERVICE, GITHUB_PAT_ACCOUNT}`. Never logs the token. |
| [`github/redact.rs`](src-tauri/src/github/redact.rs) | Token-level redaction (AWS account IDs → masked, ARNs → truncated, access keys / GitHub PATs blanked, credential-keyword lines dropped). |
| [`github/bundle.rs`](src-tauri/src/github/bundle.rs) | Builds the redacted diagnostic bundle from the event log + app/OS metadata. Capped at 64 KB. |
| [`github/client.rs`](src-tauri/src/github/client.rs) | `reqwest::blocking` Issues API client + `Transport` trait so tests can inject a fake. Error-code mapping (401/403 + rate-limit header → `TokenInvalid`/`RateLimited`, 429 → `RateLimited`, others → `Server(u16)`). |
| [`github/storage.rs`](src-tauri/src/github/storage.rs) | SQLite read/write for the `finding_tickets` table and the `github_findings_repo` setting. |

**Migration**

`src-tauri/migrations/0009_github_integration.sql` — `finding_tickets`
(PRIMARY KEY on `finding_id`, indexed by `(aws_account_id, created_at DESC)`)
plus a `github_findings_repo` row in the existing `settings` table.

**Keychain registry**

`keychain::GITHUB_PAT_SERVICE = "cloudsaw.github_pat"`, registered in
the panic-wipe enumeration so Contract 11's panic action removes the
PAT alongside every other CloudSaw secret.

**IPC surface** (registered in `src-tauri/src/lib.rs`):

- `github_get_settings`, `github_set_token`, `github_clear_token`,
  `github_set_findings_repo`, `github_generate_token_url`
- `github_prepare_error_report`, `github_submit_error_report`,
  `github_browser_fallback_for_error`
- `github_prepare_finding_ticket`, `github_submit_finding_ticket`,
  `github_browser_fallback_for_finding`
- `github_get_finding_ticket`, `github_list_finding_tickets`

**Frontend additions**

- `src/components/ErrorReportDialog.tsx` — the "Something went wrong"
  modal with "Save bundle", "File bug report", "Configure token", and
  the `security@cloud-saw.com` block.
- `src/components/SubmissionPreviewModal.tsx` — the shared
  preview-before-submit modal used by both the error-report flow AND
  the finding-ticket flow. Always renders both "Submit via API" and
  "Open in browser" buttons.
- `src/components/ErrorBoundary.tsx` — top-level boundary that catches
  render exceptions and offers the error report dialog.
- `src/routes/Settings.tsx` — new GitHub section: PAT input
  (password-style), "Generate token" link, repo input, error-report
  repo display, security contact display.
- `src/routes/dashboard/FindingsView.tsx` — per-finding "Create GitHub
  ticket" button, linked-ticket display ("Tracked in {repo}#N"), and
  the preview modal wiring.
- `src/App.tsx` — wraps the post-unlock app in `ErrorBoundary` and
  exposes a manual `openReport(notes?)` plumbed through to the
  Dashboard's load-error block.

---

## Acceptance criteria — Happy Path

| QA item | Verified by | Result |
|---|---|---|
| With a valid token, "File bug report" files an issue on the CloudSaw repo via the API, but only after the user reviews the exact submission content. | `happy_with_valid_token_file_bug_files_via_api_after_review` exercises `prepare_error_report` → `create_issue_with(FakeTransport)` and asserts the captured request matches the preview body verbatim. The UI's `SubmissionPreviewModal` passes the same `IssuePreview` back to `githubSubmitErrorReport`, so what the user sees IS what's submitted. | ✅ |
| With no token, "File bug report" copies the bundle and opens a prefilled GitHub new-issue page; no token is required. | `happy_no_token_browser_fallback_url_is_built_with_prefilled_content` confirms the fallback URL builder needs no PAT and contains `title=` + `body=`. The `SubmissionPreviewModal` invokes it and also writes the body to the clipboard. | ✅ |
| With a token configured, the user can still choose browser submission. | The preview modal renders both "Submit via API" AND "Open in browser" buttons regardless of `tokenConfigured`. `security_browser_fallback_url_never_contains_token` confirms the fallback URL is identical with/without a token. | ✅ |
| A per-finding "Create ticket" files a GitHub issue on the user-selected repo, prefilled with finding and remediation detail, and stores the finding↔issue link. | `happy_finding_ticket_files_on_user_selected_repo_with_remediation` covers preview content, transport call, and the persistent link via `gh_storage::upsert_finding_ticket`. | ✅ |
| The Settings "Generate token" helper opens the GitHub fine-grained-token page. | `happy_generate_token_url_points_at_finegrained_settings_page`. The Settings UI's `openTokenPage` calls `ipc.githubGenerateTokenUrl` and `window.open(url, "_blank", "noopener,noreferrer")`. | ✅ |

## Acceptance criteria — Error States

| QA item | Verified by | Result |
|---|---|---|
| Invalid/expired token → actionable error pointing to Settings; browser fallback still works. | `error_invalid_token_yields_actionable_code_browser_fallback_still_works`. The error code `github_token_invalid` maps to `github.error.token_invalid` ("Open Settings → GitHub and replace it with a fresh fine-grained PAT"). The fallback URL is built without a token regardless. | ✅ |
| No findings-ticket target repo set → user is prompted to select one. | `error_no_findings_repo_yields_dedicated_code`. The UI's `FindingTicketRow` disables "Create ticket" with `findings_repo.none` copy when no repo is selected. | ✅ |
| Finding already has a linked ticket → existing link shown; no duplicate. | `error_duplicate_ticket_is_rejected_existing_link_remains` exercises `submit_finding_ticket` against a pre-seeded link and asserts `DuplicateTicket`. The UI's `FindingTicketRow` renders the linked-ticket card instead of the CTA when `ticket` is non-null. | ✅ |
| GitHub API rate limit / network failure → clear error; retry or browser fallback available. | `error_rate_limit_and_network_failures_have_distinct_codes`. Each error variant maps to a localized message in `en.json` and the preview modal keeps the "Open in browser" button enabled. | ✅ |
| Very large diagnostic bundle → bounded; still reviewable before submission. | `error_very_large_bundle_is_bounded_below_max_bytes`. 5,000 events seeded; rendered body stays under `MAX_BUNDLE_BYTES = 64 KB`. The preview modal renders the full body in a scrollable `<pre>`. | ✅ |

## Acceptance criteria — Responsiveness

| QA item | Verified by | Result |
|---|---|---|
| The error dialog appears promptly on an unhandled error. | `ErrorBoundary` is a React class component with synchronous `getDerivedStateFromError`; the boundary renders its fallback in the next render pass with no awaitable work — confirmed by the dialog opening directly from a manual click in `lock-error-report` / `render-error-report`. | ✅ |
| Issue submission reports progress and resolves without an indefinite hang. | `responsiveness_prepare_error_report_returns_promptly_with_many_events` covers prep with 2k events under 2s. The submit path is gated by `reqwest`'s 30s total timeout (`client.rs`) so an unresponsive GitHub can't hang the UI. | ✅ |
| The submission-preview modal renders the full content clearly. | The modal's `<pre>` body has `max-h-72 overflow-auto whitespace-pre-wrap`; the title, labels, destination, and body are each rendered in their own block. | ✅ |

## Acceptance criteria — State Transitions

| QA item | Verified by | Result |
|---|---|---|
| No token → token configured → direct submission becomes available. | `state_no_token_then_token_configured_then_settings_reflect_it`. The `SubmissionPreviewModal` enables "Submit via API" iff `tokenConfigured`. | ✅ |
| Token configured → per-report choice → browser submission used instead. | The preview modal's "Open in browser" button is always enabled regardless of token state. Confirmed by the UI structure; `happy_no_token_browser_fallback_url_is_built_with_prefilled_content` confirms the URL builder works token-less. | ✅ |
| Finding with no ticket → "Create ticket" → finding linked to issue #N. | `state_finding_no_ticket_then_link_then_get_returns_link`. | ✅ |
| Token configured → token revoked externally → next submission errors actionably. | `error_invalid_token_yields_actionable_code_browser_fallback_still_works` simulates this exact transition via `FakeOutcome::Err(TokenInvalid)`. The Settings UI's "Configure GitHub token" copy directs the user to replace the token. | ✅ |

## Security Check

| QA item | Verified by | Result |
|---|---|---|
| Exactly one PAT is used, stored only in the OS keychain as `cloudsaw.github_pat`; absent from SQLite, config files, logs, and URLs. | `security_pat_lives_only_in_keychain_registry_includes_it_for_panic_wipe` writes the PAT via the public API, then SELECT-everys every non-system table and asserts no `ghp_` / `github_pat_` substring appears. The PAT also crosses IPC via `Zeroizing<String>` (`pat::get`) so it never sits in a long-lived buffer. | ✅ |
| Direct API submission shows the complete submission content before sending; proceeds only on explicit user action. | The IPC bridge splits `prepare_*` and `submit_*` into two calls; the UI's `SubmissionPreviewModal` is the bridge between them. The user must click "Submit via GitHub API" — no auto-submit path exists. `happy_with_valid_token_file_bug_files_via_api_after_review` asserts the captured request body equals the preview body verbatim. | ✅ |
| Browser fallback always available, including when a token is configured. | The preview modal renders the "Open in browser" button without checking `tokenConfigured`. `security_browser_fallback_url_never_contains_token` confirms the URL builder takes no token argument. | ✅ |
| Error dialog displays `security@cloud-saw.com`. | `security_security_contact_is_exposed_as_constant`. The dialog renders the address with a "Copy" button. | ✅ |
| Diagnostic bundle redacted: account IDs masked, ARNs truncated, no credentials/tokens/API keys. | `security_diagnostic_bundle_is_redacted_no_credentials_or_account_ids` exercises every redaction rule (account ID, ARN with path, AWS access key, GitHub PAT, credential-keyword line). | ✅ |
| No issue body contains credentials or unmasked account identifiers. | `security_finding_ticket_body_redacts_account_ids` confirms `prepare_finding_ticket` redacts the finding description. Free-form user notes go through `redact::redact_block` in `bundle::build`. | ✅ |
| Findings tickets are filed only against an explicitly user-selected repo. | `security_findings_ticket_only_files_on_user_selected_repo`. The IPC takes the repo as an explicit `RepoSelection` argument; the UI passes `github.findings_repo` (the user's saved selection) — no inference. | ✅ |
| Every GitHub ticket creation is recorded in the event log. | `security_ticket_creation_is_recorded_in_event_log`. Both `submit_error_report` and `submit_finding_ticket` call `eventlog::record_event(GithubTicketCreated, …)`. | ✅ |

---

## Test run summary

Full Rust workspace (lib + integration tests, serialized — `-j 1
--test-threads=1`):

```
running 124 tests   (cloudsaw_lib unit)                           → 124/124
running 24  tests   (accounts_test)                                → 24/24
running 17  tests   (applock_test)                                 → 17/17
running 11  tests   (auth_test)                                    → 11/11
running 20  tests   (findings_test)                                → 20/20
running 26  tests   (knowledgebase_test)                           → 26/26
running 5   tests   (migrations_test)                              → 5/5
running 19  tests   (qa05_test)                                    → 19/19
running 23  tests   (qa06_test)                                    → 23/23
running 18  tests   (qa10_test)                                    → 18/18
running 25  tests   (qa11_test)                                    → 25/25
running 20  tests   (qa12_test)        ← new this contract         → 20/20
running 20  tests   (scanner_test)                                 → 20/20
running 9   tests   (scheduler_test)                               → 9/9
running 16  tests   (terraform_test)                               → 16/16

Total: 377 / 377 ✅
```

Frontend gates:

```
$ tsc --noEmit                                                   → clean
$ vite build                                                     → 359.60 kB bundle, clean
```

---

## Operator-driven checks

These items can't be cleanly asserted from a Rust integration test —
they need a real OS keychain populated, a real PAT, or the actual
`api.github.com` endpoint. They're enumerated here so a release
manager can tick them off before tagging:

1. **PAT round-trip on each platform.** Add a fine-grained PAT in
   Settings → GitHub. Quit the app, reopen, confirm "Token configured."
   shows in the GitHub section. Run "Remove token" and confirm the
   status flips back to "No token configured."
2. **Real submission to the CloudSaw repo.** With a valid PAT scoped
   to `Issues: write` on the CloudSaw repo, trigger the error dialog
   manually (`render-error-report` from a forced exception, or the
   lock-error fallback), pick "File bug report," review the preview,
   click "Submit via GitHub API." Confirm the new issue appears on
   GitHub at the URL returned in the success dialog and that the
   redacted bundle contains no `111122223333` / `arn:aws:iam::…`
   substrings.
3. **Browser fallback while a token is configured.** With a PAT set,
   pick "Open in browser instead" in the preview modal. Confirm the
   GitHub new-issue page opens prefilled. Confirm the URL contains
   neither `Authorization=` nor any `ghp_…` / `github_pat_…`
   substring.
4. **Generate-token link.** Click "Generate token on GitHub" in
   Settings; confirm the OS default browser opens
   `https://github.com/settings/personal-access-tokens/new`.
5. **Token revoked externally.** Revoke the PAT in GitHub's UI while
   CloudSaw is open. Trigger a submission. Confirm the error message
   advises opening Settings → GitHub and replacing the PAT, and that
   "Open in browser" still works.
6. **Findings-ticket flow.** Set a `findings_repo` in Settings, open
   the Dashboard → a finding's detail, click "Create GitHub ticket,"
   review the preview, submit. Confirm:
   - The issue is filed on the chosen repo, not the CloudSaw repo.
   - The body contains the rule, severity, masked account, and (if
     present) the KB remediation block.
   - The finding's detail panel now shows "Tracked in {repo}#N" with a
     link.
   - Clicking "Create GitHub ticket" again surfaces the existing link
     rather than filing a duplicate.
7. **Panic interaction.** Configure a PAT, then run Settings → Panic.
   Confirm the PAT is removed from the OS keychain (verify via
   Keychain Access / `secret-tool search` / Credential Manager) and
   that the next CloudSaw install starts with "No token configured."
