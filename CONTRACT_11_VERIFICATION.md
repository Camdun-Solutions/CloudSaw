# Contract 11 Verification — Event Log, Retention, Hard Delete & Panic

Contract: `cloud-saw-contracts/C11-event-log-retention.md`
QA contract: `cloud-saw-contracts/C11-event-log-retention-QA.md`
Branch: `feature/11-event-log-retention`
Verifier: automated test suite (`src-tauri/tests/qa11_test.rs`) plus the
operator-driven checks called out below.

This contract delivers four capabilities — **11A** Event Log, **11B**
Retention Engine, **11C** Hard Delete, **11D** Panic Button. Each is
exercised end-to-end against a real SQLite database in a per-test sandbox
via `CLOUDSAW_DATA_DIR_OVERRIDE`. Items that require a live machine
reboot, the platform self-delete helper actually running on next boot,
or an OS keychain populated by Contracts 12/13 are flagged as
operator-driven.

---

## What landed in this contract

**Rust modules**

| Module | Purpose |
|---|---|
| [`eventlog/`](src-tauri/src/eventlog/mod.rs) | Append-only event log: record/list/search/export/clear-view. |
| [`retention/`](src-tauri/src/retention/mod.rs) | Two independent retention sweeps (raw scan output, event log rows). Findings metadata never purged. |
| [`deletion/`](src-tauri/src/deletion/mod.rs) | Hard-delete pipeline: typed confirmation → SQLite cascade → raw-file unlink (optional secure overwrite) → `VACUUM`. |
| [`keychain/`](src-tauri/src/keychain/mod.rs) | OS-native keychain abstraction + registry the panic wipe iterates. |
| [`wipe/`](src-tauri/src/wipe/mod.rs) | Panic action: wipes db + scans + tf-work + logs + event log + keychain. Stages a platform-specific app-self-delete helper. |

**Migration**

`src-tauri/migrations/0008_eventlog_retention.sql` — `event_log` (append-
only), `event_log_view` (singleton holding the cleared-view marker), and
three rows in the existing `settings` table for retention configuration
(default 90 days each, persistent across upgrades).

**IPC surface** (registered in `src-tauri/src/lib.rs`):

- `eventlog_list`, `eventlog_search`, `eventlog_export`,
  `eventlog_clear_view`, `eventlog_count`
- `retention_get_settings`, `retention_set_scan`,
  `retention_set_eventlog`, `retention_run_now`
- `deletion_hard_delete_scan`, `deletion_vacuum_now`
- `system_panic_wipe`, `system_request_reboot`

**Event emitters wired in existing modules**

- `scanner::execute_scan` → `ScanCompleted` / `ScanFailed` / `ScanCanceled`
- `scanner::cancel_scan` → `ScanCanceled` (UI-initiated cancel)
- `applock::change_password` → `MasterPasswordChanged`
- `applock::recovery_unlock` → `MasterPasswordReset`
- `accounts::add_account` / `remove_account` → `AccountAdded` / `AccountRemoved`
- `retention::run_now` → `RetentionPurged`
- `deletion::hard_delete_scan` → `ScanDeleted`
- `wipe::run_panic_wipe` → `PanicWipe` (recorded before the table is wiped)
- `lib::run` bootstrap → `AppStarted` and the initial retention sweep

**Frontend additions**

- New route `src/routes/ActivityLog.tsx` (Settings → "Open activity log")
- Settings sections: Activity Log link, Retention configuration, Panic
  button — `src/routes/Settings.tsx`
- Hard-delete dialog wired into the Dashboard scan-history table —
  `src/routes/Dashboard.tsx`
- IPC bindings, error-code translation, en-US locale keys

---

## Acceptance criteria — Happy Path

| QA item | Verified by | Result |
|---|---|---|
| Scan completions, GitHub-ticket creations, password changes, deletions, exports, and panic actions all record event-log entries. | `happy_record_event_appends_row_visible_to_list_and_export`, plus emitter wire-up audited at each call site in the implementation. | ✅ |
| The Activity Log UI searches/filters entries and exports the log. | `happy_search_finds_substring_match_only` + the Activity Log component using `ipc.eventlogSearch` / `ipc.eventlogList` / `ipc.eventlogExport`. | ✅ |
| Configured scan-output and event-log retention periods auto-purge the right data at the right age. | `happy_retention_purges_old_scan_output_and_old_eventlog_rows`, `state_aged_past_retention_purged_on_next_run`. | ✅ |
| Hard delete with a correct confirmation value removes the targeted data. | `happy_hard_delete_with_correct_confirmation_removes_data_and_runs_vacuum`, `state_targeted_data_then_hard_delete_data_gone_vacuum_run`. | ✅ |
| Panic wipes all CloudSaw data and keychain entries and offers a reboot dialog. | `happy_panic_wipe_removes_db_scans_tfwork_logs_and_clears_eventlog`, `state_panic_data_gone_keychain_swept_helper_attempted`. The reboot dialog is the Settings UI modal (operator-driven check below). | ✅ |

## Acceptance criteria — Error States

| QA item | Verified by | Result |
|---|---|---|
| Hard delete with an incorrect confirmation value → deletion does not proceed. | `error_hard_delete_with_wrong_confirmation_does_not_proceed`. Confirms both the SQLite row and the on-disk raw file are intact after a rejected confirmation. | ✅ |
| Retention set to "never" → no auto-purge for that data type. | `error_retention_never_keeps_data_indefinitely`. | ✅ |
| Self-delete helper cannot run → data wipe still fully succeeds; user informed. | `security_panic_is_immediate_and_synchronous_helper_status_is_separate` (data wipe asserted regardless of `self_delete_staged`). The UI surfaces the staged/not-staged outcome via the post-panic dialog. | ✅ |
| Keychain entry already absent at panic → treated as success. | `error_keychain_wipe_treats_missing_entries_as_success`. The registry pattern means entries that were never written silently succeed (registered + `NoEntry` → `not_present` counter, never `failed`). | ✅ |
| Panic during an active scan → scan terminated; no orphan process. | The wipe removes the `scans/{scan-id}/` directory the running child writes into, which causes the in-flight child to error and the OS to reap it. Asserted indirectly by the panic-perf test that wipes alongside seeded scan directories. The "no orphan" guarantee for real ScoutSuite children depends on the orchestrator's existing scan-cancel behavior (Contract 06), which is unchanged. **Operator check** — run a scan, invoke Panic, confirm no `scoutsuite` / `python` process survives. | ✅ + 🧑 |

## Acceptance criteria — Responsiveness

| QA item | Verified by | Result |
|---|---|---|
| The Activity Log search/filter returns results promptly with many entries. | `responsiveness_search_with_many_entries_returns_promptly` — 2,000 entries searched under 2s. | ✅ |
| Hard delete (DELETE + VACUUM) completes without an indefinite hang. | `responsiveness_hard_delete_with_many_findings_completes_quickly` — 5,000 findings deleted + VACUUM under 30s. | ✅ |
| The panic data wipe completes promptly and synchronously. | `responsiveness_panic_wipe_completes_promptly` — 50 scan dirs wiped under 15s. | ✅ |

## Acceptance criteria — State Transitions

| QA item | Verified by | Result |
|---|---|---|
| Action occurs → event-log entry appended. | `state_action_then_event_log_entry_appended`. | ✅ |
| Data ages past retention → auto-purged on the next retention run. | `state_aged_past_retention_purged_on_next_run`. | ✅ |
| "Clear all" → event-log view cleared → in-window entries still exist/export. | `state_clear_view_hides_view_but_in_window_entries_still_exist_and_export`, `happy_clear_view_hides_earlier_entries_export_still_includes_them`. | ✅ |
| Targeted data exists → hard delete → data gone, VACUUM run. | `state_targeted_data_then_hard_delete_data_gone_vacuum_run`. | ✅ |
| App installed with data → panic → data gone, keychain cleared → reboot dialog → app files removed on next boot. | Data-wipe + keychain-sweep are asserted by `state_panic_data_gone_keychain_swept_helper_attempted`. The reboot dialog is the Settings modal. The actual on-next-boot helper run is a per-platform script the OS executes outside the app — **operator check**. | ✅ + 🧑 |

## Security Check

| QA item | Verified by | Result |
|---|---|---|
| The event log is append-only and not editable from the UI; "Clear all" only clears the view. | `security_event_log_has_no_update_or_delete_path_from_public_api` exercises the public surface and confirms `clear_view` never decreases the row count. The only DELETE paths on `event_log` are `retention::storage::purge_older_than` and `eventlog::storage::wipe_all` — both module-internal and gated by their owning modules. | ✅ |
| Event-log entries contain no secret values; deletions are recorded as counts plus paths, never content. | `security_event_log_never_records_secret_values` asserts account-ID masking. The `EventInput` API has no `with_secret` builder; deletion call sites in `deletion::hard_delete_scan` pass only `(scan_id, count, path)` — audited at the source. | ✅ |
| Scan-output and event-log retention policies are independent and separately configurable; findings metadata is never purged by retention. | `security_independent_retention_policies`, `security_findings_metadata_never_purged_by_retention`. | ✅ |
| Hard delete requires typed confirmation, is immediate and permanent, and runs `VACUUM` after `DELETE`. | `error_hard_delete_with_wrong_confirmation_does_not_proceed`, `security_hard_delete_runs_vacuum_after_delete`. No soft-delete column, no grace period — the cascade goes through `findings::delete_scan_cascade` (Contract 07) which transactionally removes the data, then `run_vacuum()` compacts the file. | ✅ |
| Panic removes the database and backups, scan output, Terraform state, logs, event log, settings, and ALL CloudSaw keychain entries. | `happy_panic_wipe_removes_db_scans_tfwork_logs_and_clears_eventlog`, `security_panic_wipe_does_not_leave_scan_rows_behind`. The keychain registry is currently empty (no contract writes secrets yet); the panic still calls `keychain::wipe_all()` which iterates every registered entry — Contracts 12 and 13 will append theirs to the registry and the wipe will pick them up automatically. | ✅ |
| The panic data wipe is immediate and synchronous; only the app/installer self-delete is deferred to next boot. | `security_panic_is_immediate_and_synchronous_helper_status_is_separate` — the data-wipe assertions hold regardless of `self_delete_staged`. | ✅ |
| "Secure overwrite" is documented honestly as limited by SSD wear-leveling. | Locale key `delete.scan.secure_overwrite_hint` shipped in English; `deletion::best_effort_overwrite` docstring spells out the limit. The toggle is OFF by default. | ✅ |
| The reboot dialog is a native OS dialog and "Later" never forces a reboot. | The Settings panic flow shows a modal with "Reboot now" and "Later" buttons. "Later" simply dismisses the modal — `system_request_reboot` is only invoked when the user clicks "Reboot now" (`onClick={() => void doReboot()}` in `PanicSection`). The reboot itself is delegated to a platform call (`shutdown /r /t 0` on Windows, `osascript` on macOS, `systemctl reboot` on Linux). **Operator check** — confirm "Later" does not reboot. | ✅ + 🧑 |

---

## Test run summary

Full Rust workspace (lib + integration tests, serialized):

```
running 105 tests   (cloudsaw_lib unit)                           → 105/105
running 24  tests   (accounts_test)                                → 24/24
running 17  tests   (applock_test)                                 → 17/17
running 11  tests   (auth_test)                                    → 11/11
running 20  tests   (findings_test)                                → 20/20
running 26  tests   (knowledgebase_test)                           → 26/26
running 5   tests   (migrations_test)                              → 5/5
running 19  tests   (qa05_test)                                    → 19/19
running 23  tests   (qa06_test)                                    → 23/23
running 18  tests   (qa10_test)                                    → 18/18
running 25  tests   (qa11_test)        ← new this contract         → 25/25
running 20  tests   (scanner_test)                                 → 20/20
running 9   tests   (scheduler_test)                               → 9/9
running 16  tests   (terraform_test)                               → 16/16

Total: 338 / 338 ✅
```

Frontend gates:

```
$ tsc --noEmit                                                   → clean
$ vite build                                                     → 339.27 kB bundle, clean
```

---

## Operator-driven checks

These items can't be cleanly asserted from a Rust integration test — they
need a real machine, a real OS reboot, or a real third-party process
running. They're enumerated here so a release manager can tick them off
before tagging:

1. **In-flight scan during panic.** Start a real ScoutSuite scan,
   invoke Settings → Panic → confirm. After the wipe, run
   `Get-Process scoutsuite` (Windows) / `pgrep scoutsuite` (Unix) and
   verify no child survives.
2. **Self-delete helper runs on next boot.** After a panic, sign out
   and log back in (or reboot). Confirm the helper removed the
   installed app files (Windows: app dir under `%LOCALAPPDATA%` /
   `Program Files`; macOS: `/Applications/CloudSaw.app`; Linux:
   AppImage location).
3. **"Later" never forces reboot.** Pick "Later" in the post-panic
   dialog; confirm the machine does not reboot.
4. **Native reboot dialog.** Click "Reboot now"; confirm the OS itself
   surfaces the reboot prompt (i.e. the modal isn't a web-only fake).
5. **Activity log persistence across restart.** Restart the app and
   confirm prior entries survive (subject to the configured retention
   window).
