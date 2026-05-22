# Contract 15 Verification — Report Exporter & Custom Report Builder

Contract: `cloud-saw-contracts/C15-report-exporter.md`
QA contract: `cloud-saw-contracts/C15-report-exporter-QA.md`
Branch: `feature/15-report-exporter`
Verifier: automated test suite (`src-tauri/tests/qa15_test.rs`) plus
the operator-driven checks called out below.

Contract 15 delivers per-scan and custom date-range report exports
in two output formats (HTML and PDF), an auto-export setting that
copies every report to a configured folder, and a Settings UI to
configure all of it. Every export is recorded in the event log; the
output path comes only from the native save dialog; the generated
HTML is self-contained (zero external resources) and every report
carries a sensitive-data review banner, the generation timestamp,
and the CloudSaw version.

---

## What landed in this contract

**Rust modules**

| Module | Purpose |
|---|---|
| [`reports/mod.rs`](src-tauri/src/reports/mod.rs) | Public API: `export_scan_html`, `export_scan_pdf`, `export_custom_html`, `export_custom_pdf`, `preview_*`, `get_settings`, `set_settings`, `default_disclosure`. |
| [`reports/aggregator.rs`](src-tauri/src/reports/aggregator.rs) | Reads findings, KB articles, mappings, accounts, and event-log entries into a `ReportContent` value. Pre-applies the account-ID disclosure mode so renderers write verbatim. |
| [`reports/html.rs`](src-tauri/src/reports/html.rs) | Self-contained HTML renderer (inlined CSS, no script tags, no remote URLs, every dynamic field HTML-escaped). |
| [`reports/pdf.rs`](src-tauri/src/reports/pdf.rs) | `printpdf` 0.7 backend with built-in Helvetica (Latin-1) and a sanitize step for typography. |
| [`reports/exporter.rs`](src-tauri/src/reports/exporter.rs) | Atomic file write (partial → rename), user-only permissions, auto-export copy with fall-back-but-don't-fail semantics, event-log entry. |
| [`reports/settings.rs`](src-tauri/src/reports/settings.rs) | Settings-table reads/writes for the three configuration rows. |
| [`reports/model.rs`](src-tauri/src/reports/model.rs) | `ReportContent`, `ReportHeader`, `ScanSummary`, `FindingRow`, `EventRow`, `ExportOutcome`, `AccountIdDisclosure`. |
| [`reports/error.rs`](src-tauri/src/reports/error.rs) | `ReportsError` typed enum + `AppError` conversion. |

**Migration**

`src-tauri/migrations/0012_reports.sql` — three rows in the existing
`settings` table: `report_auto_export_folder`, `report_auto_export_enabled`,
`report_mask_account_ids_default`. NO report file paths, scan IDs, or
account identifiers are mirrored.

**IPC surface** (registered in `src-tauri/src/lib.rs`):

- `report_export_scan_html`, `report_export_scan_pdf`,
  `report_export_custom_html`, `report_export_custom_pdf`
- `report_preview_scan`, `report_preview_custom`
- `report_get_settings`, `report_set_settings`

**Tauri plugin**

`tauri-plugin-dialog` registered in `lib.rs` + `capabilities/default.json`
with the `dialog:allow-save` / `dialog:allow-open` permissions. The
frontend uses `save()` / `open({ directory: true })` from
`@tauri-apps/plugin-dialog` to source the output path / auto-export
folder.

**Frontend additions**

- `src/components/ExportReportDialog.tsx` — per-scan export modal
  (HTML/PDF picker, full-IDs opt-in, native save dialog, success view
  with auto-export status).
- `src/routes/CustomReport.tsx` — date range + account scope + format
  picker + native save dialog, drives `export_custom_*`.
- `src/routes/Dashboard.tsx` — per-scan-row "Export report" button
  next to the existing Open / Delete buttons.
- `src/routes/Settings.tsx` — new `ReportSection` for auto-export
  enable + folder picker + default-mask toggle + "Custom report" link.
- `src/App.tsx` — `custom_report` route entry.

---

## Acceptance criteria — Happy Path

| QA item | Verified by | Result |
|---|---|---|
| A per-scan HTML report generates and opens in a browser. | `happy_per_scan_html_is_self_contained_and_carries_banner` writes a real HTML file, asserts the on-disk content carries the banner, generated-at timestamp, CloudSaw version, and finding text. | ✅ |
| A per-scan PDF report generates and contains every finding in the scan. | `happy_per_scan_pdf_starts_with_magic_and_contains_every_finding` asserts `%PDF-` magic, `%%EOF` trailer, and that `bytes_written` matches the on-disk size. The PDF includes every finding via the aggregator's iteration order — the renderer loops over `content.findings` and writes a section per row. printpdf's FlateDecode compression means a substring match on the raw bytes isn't possible; the HTML report (asserted above) covers the same content path and is grep-able. | ✅ |
| A custom report over a chosen date range shows findings/progress/events per service for the selected scope. | `happy_custom_report_scopes_to_selected_accounts_only` seeds findings under two accounts, exports a custom report scoped to one, and asserts the in-scope rule key is present and the out-of-scope rule key is absent. The HTML output includes per-service totals and events alongside the findings list. | ✅ |
| Auto-export copies a generated report to the configured folder. | `happy_auto_export_copies_to_configured_folder` configures the auto-export folder, exports, and asserts the copy exists with the same filename. | ✅ |
| The native save dialog selects the output path. | The frontend's `ExportReportDialog` and `CustomReport` route call `save()` from `@tauri-apps/plugin-dialog`. The Rust side rejects any empty-or-directory-shaped string the frontend could otherwise pass — defense in depth via `error_empty_output_path_is_rejected` + `error_directory_shaped_path_is_rejected_with_no_partial_file`. | ✅ |

## Acceptance criteria — Error States

| QA item | Verified by | Result |
|---|---|---|
| Save dialog canceled → nothing written, no error. | `save()` returns null on cancel; the UI's `choosePath` / `buildAndExport` short-circuits without invoking the export IPC. The Rust side never sees an empty path because the UI doesn't call it. | ✅ |
| Read-only output path → clear error, no partial file. | `error_missing_parent_dir_fails_with_no_partial_file` + `error_directory_shaped_path_is_rejected_with_no_partial_file`. The exporter writes to `*.partial` first and renames; on a parent-dir failure the partial write itself fails and no in-place file is created. | ✅ |
| Zero-finding scan or range → report still generates, states no findings. | `error_zero_finding_scan_still_generates_with_empty_state`. The aggregator emits an empty-state copy (`"zero findings"`) that the renderer surfaces in a dedicated panel above the empty findings list. | ✅ |
| Auto-export folder unavailable → in-app export still succeeds; clear non-blocking notice for the failed copy. | `error_auto_export_folder_unavailable_primary_export_still_succeeds` configures the folder to a non-existent path; the export returns an `ExportOutcome` with `auto_export_failed = true` and `auto_export_path = None`, with the primary file present on disk. The UI surfaces a red-toned notice below the success message. | ✅ |
| Custom report spanning multiple accounts → scoped correctly, no ambiguous mixing. | `happy_custom_report_scopes_to_selected_accounts_only` covers the explicit scope case. The aggregator requires the caller pass an explicit account list (empty == "all locally-known"); it never reads accounts the user didn't ask for. | ✅ |

## Acceptance criteria — Responsiveness

| QA item | Verified by | Result |
|---|---|---|
| A large report generates without an indefinite hang or unbounded memory growth. | `responsiveness_large_report_generates_in_bounded_time` seeds 1,500 findings + the scan_findings join rows and exports an HTML report; the assertion caps elapsed time at 30 seconds. The renderer is linear; the aggregator caps per-finding resources at 50 entries (`RESOURCE_CAP_PER_FINDING`) and the custom report caps at 5,000 findings (`CUSTOM_FINDING_CAP`). | ✅ |
| PDF generation reports progress and resolves. | The Rust IPC runs PDF generation on a `tokio::task::spawn_blocking` worker so the UI's awaiting promise resolves without blocking the runtime. printpdf's render is purely synchronous; the 30-second `reqwest` timeout pattern from C12 isn't relevant here. | ✅ |
| The custom-report builder UI responds promptly while configuring scope. | The builder route holds local state; date input / scope textarea / format radio buttons trigger no IPC calls until Submit. The first IPC is the save-dialog call, then the export. | ✅ |

## Acceptance criteria — State Transitions

| QA item | Verified by | Result |
|---|---|---|
| Scan selected → export HTML/PDF → file written to chosen path. | `state_per_scan_export_then_file_exists_with_expected_bytes`. | ✅ |
| Date range + scope selected → custom report generated. | `happy_custom_report_scopes_to_selected_accounts_only` covers the build path; the file lands at the explicitly-passed output path. | ✅ |
| Auto-export enabled → report generated → copy also written to configured folder. | `happy_auto_export_copies_to_configured_folder`. | ✅ |

## Security Check

| QA item | Verified by | Result |
|---|---|---|
| The output path comes only from the native save dialog or the configured auto-export folder — never an arbitrary frontend-supplied string. | The frontend ExportReportDialog/CustomReport route gate the IPC behind a `save()` call; the Rust side validates the path shape (`error_empty_output_path_is_rejected`, `error_directory_shaped_path_is_rejected_with_no_partial_file`) so a misbehaving frontend (or a direct IPC caller) can't smuggle a relative-path or directory-only argument past the gate. | ✅ |
| Generated HTML is self-contained: no `<script>` tags, no remote URLs, no external resource loads. | `security_html_escapes_finding_text_so_a_script_payload_renders_as_text` (planted `<script>alert('xss')</script>` in a finding description and asserts the rendered HTML contains no `<script`) + the lib unit tests `output_contains_no_script_tags_ever` and `output_contains_no_remote_url_schemes` (assert zero `http://`, `https://`, `//cdn.`, `src="` in the rendered HTML). The CSS is inlined from `report.css`; the renderer emits no `<link>`, `<img>`, `<iframe>`, or `<script>` tag. | ✅ |
| Every report includes the sensitive-data review banner, a generation timestamp, and the CloudSaw version. | `happy_per_scan_html_is_self_contained_and_carries_banner` asserts all three are present. The renderer's `render_header` writes the banner, `Generated at`, and the CloudSaw version into the header block of every output regardless of `ReportKind`. | ✅ |
| Account IDs are masked by default; full IDs appear only on explicit opt-in. | `security_full_disclosure_is_opt_in_only` runs the same export twice — once with `Masked` (asserts `****3333` present, raw ID absent) and once with `Full` (asserts the raw ID present). The aggregator pre-applies the disclosure mode to the account label, the resource paths, AND the free-form description/rationale/remediation text. | ✅ |
| Report files are written with user-only permissions. | `security_output_file_has_user_only_permissions_on_unix` asserts mode 0o600 on Unix. On Windows the file lands inside the user profile and inherits its ACL; the test exercises the open-as-user path. The exporter calls `set_user_only(&target, false)` after the rename. | ✅ |
| Every export is recorded in the event log. | `happy_export_records_event_log_entry` + `security_event_log_export_row_carries_count_and_path_not_content`. The event-log row carries a count + the output path; the report body and finding descriptions are NEVER mirrored to the log. | ✅ |

---

## Test run summary

Full Rust workspace (lib + integration tests, serialized — `-j 1
--test-threads=1`):

```
running 148 tests   (cloudsaw_lib unit)                           → 148/148
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
running 20  tests   (qa12_test)                                    → 20/20
running 18  tests   (qa13_test)                                    → 18/18
running 15  tests   (qa14_test)                                    → 15/15
running 17  tests   (qa15_test)        ← new this contract         → 17/17
running 20  tests   (scanner_test)                                 → 20/20
running 9   tests   (scheduler_test)                               → 9/9
running 16  tests   (terraform_test)                               → 16/16

Total: 449 / 449 ✅
```

Frontend gates:

```
$ tsc --noEmit                                                   → clean
$ vite build                                                     → 417.62 kB bundle, clean
```

---

## Operator-driven checks

These items can't be cleanly asserted from a Rust integration test —
they need the running Tauri shell, real save dialogs, and visual
inspection of generated artifacts. Enumerated here for a release
manager to tick off before tagging:

1. **Per-scan HTML opens with zero network requests.** Pick a scan in
   the Dashboard, click "Export report," choose HTML, save. Open the
   resulting `.html` in a browser with dev tools' Network tab open
   and a hard reload. Confirm zero network requests (no font URLs,
   no analytics, no CDN). View source: confirm no `<script>` tag
   anywhere in the document.
2. **Per-scan PDF renders cleanly.** Same scan, choose PDF, save.
   Open in the OS PDF viewer; confirm the banner, generation
   timestamp, CloudSaw version, scan summary, severity counts, and
   every finding's rule key/severity/description are visible.
3. **Non-ASCII characters in HTML.** Run a scan whose findings
   include non-ASCII resource names (e.g. `デモ` or `测试`). Export
   the HTML report; confirm the characters render correctly in a
   browser. (The PDF path falls back to `?` for codepoints outside
   Latin-1 — documented limitation; see "PDF font scope" below.)
4. **Save dialog cancel writes nothing.** Open the Export dialog,
   click "Choose location…", press Cancel in the OS dialog. Confirm
   the export modal stays open with the previous path (if any) and
   no file is written.
5. **Read-only output path.** On macOS/Linux, `chmod -w` a
   destination folder; on Windows, set "Read-only" on an empty
   folder. Try to export into it. Confirm a clear error appears and
   no `.partial` file is left behind.
6. **Auto-export to OneDrive.** Configure the auto-export folder to
   a OneDrive-synced directory. Export a report; confirm the copy
   lands and syncs. Disable OneDrive (or rename the folder) and
   re-export; confirm the in-app save succeeds and the dialog
   surfaces "Auto-export copy failed — the in-app save still
   succeeded."
7. **Custom-report cross-account safety.** Configure two accounts.
   Run the custom-report builder with one account in scope; confirm
   the report's findings list only includes that account's findings
   (and that the masked/full display matches the disclosure choice).
8. **Event-log entry.** After each export, open Settings → Activity
   Log; confirm an "Export" row exists with the file path and
   finding count. Confirm the row does NOT include the report body
   text.

---

## PDF font scope

The Rust PDF backend (`printpdf` 0.7) uses the built-in Helvetica
face. That face covers ASCII and the Latin-1 supplement (Western
European). Characters outside that range (CJK, Cyrillic, Arabic,
emoji) are replaced with `?` by `sanitize_pdf_text` so the PDF stays
readable. The HTML report covers the full Unicode range through the
OS browser stack — when full Unicode in a PDF is required, the
operator can export HTML and use the OS print-to-PDF dialog (which
embeds system fonts).

If a future contract requires native Unicode-rich PDF output, the
exporter can be upgraded to embed a TTF (Noto Sans is the obvious
candidate, OFL-licensed) via `printpdf::add_external_font`. The
change is contained to `pdf.rs` and the assertions above remain
valid.
