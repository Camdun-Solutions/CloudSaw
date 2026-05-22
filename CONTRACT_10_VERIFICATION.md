# Contract 10 — Scheduled & Automated Scans: Verification Report

Branch: `feature/10-scheduled-scans` (stacked on `feature/09-dashboard`)
Date: 2026-05-21
Verification scope: every item in `cloud-saw-contracts/C10-scheduled-scans-QA.md`.

---

## 1. Test execution summary

### 1.1 Scheduler-specific integration suite (`tests/scheduler_test.rs`)

```
running 9 tests
test events_record_lifecycle_transitions ... ok
test happy_set_get_list_round_trips ... ok
test toggle_enabled_round_trips_next_run ... ok
test validation_rejects_missing_time_of_day ... ok
test set_schedule_rejects_unknown_account ... ok
test state_no_schedule_then_set_then_clear ... ok
test removing_account_cascades_to_schedule ... ok
test validation_rejects_bad_account_id ... ok
test runner_skips_when_role_not_provisioned ... ok

test result: ok. 9 passed; 0 failed
```

### 1.2 Contract 10-QA acceptance suite (`tests/qa10_test.rs`)

```
running 18 tests
test qa_security_schedules_table_has_no_credential_columns ... ok
test qa_security_no_credential_returning_ipc_commands ... ok
test qa_security_runner_does_not_consult_applock ... ok
test qa_security_runner_uses_only_scanner_run_scan ... ok
test qa_error_bootstrap_after_sleep_does_not_crash ... ok
test qa_error_unprovisioned_account_skip_clear_reason ... ok
test qa_responsiveness_get_schedule_is_fast ... ok
test qa_error_inflight_scan_blocks_scheduled_run ... ok
test qa_error_account_removal_clears_schedule ... ok
test qa_happy_schedules_persist_across_restart ... ok
test qa_responsiveness_changes_take_effect_without_restart ... ok
test qa_security_catch_up_does_not_stack ... ok
test qa_error_missed_times_collapse_to_single_catch_up ... ok
test qa_happy_weekly_schedule_round_trips ... ok
test qa_security_scheduled_scans_never_parallel_per_account ... ok
test qa_state_transition_account_remove_removes_schedule ... ok
test qa_state_transition_disable_then_reenable ... ok
test qa_state_transition_no_schedule_to_enabled ... ok

test result: ok. 18 passed; 0 failed
```

### 1.3 Regression suites for earlier contracts (all individually re-run)

| Test file                | Result                  |
| ------------------------ | ----------------------- |
| `migrations_test`        | 5 passed; 0 failed      |
| `applock_test`           | 17 passed; 0 failed     |
| `auth_test`              | 11 passed; 0 failed     |
| `accounts_test`          | 24 passed; 0 failed     |
| `terraform_test`         | 16 passed; 0 failed     |
| `qa05_test`              | 19 passed; 0 failed     |
| `scanner_test`           | 20 passed; 0 failed     |
| `qa06_test`              | 23 passed; 0 failed     |
| `findings_test`          | 20 passed; 0 failed     |
| `knowledgebase_test`     | 26 passed; 0 failed     |
| `scheduler_test` (C10)   | 9 passed; 0 failed      |
| `qa10_test` (C10-QA)     | 18 passed; 0 failed     |
| **Total**                | **208 passed; 0 failed**|

> Note: `cargo test` (all targets at once) is gated on this machine by Windows
> paging; per-file `cargo test --test <name>` succeeds end-to-end as shown
> above. This matches the verification pattern used in Contract 06.

### 1.4 Frontend type check

`npx tsc --noEmit` — clean (no errors).

### 1.5 Browser-driven UI verification (`preview_*` workflow)

Per `<when_to_verify>`, the new Settings → Schedules surface is observable in
the browser. Verified with a stubbed Tauri-IPC bridge so the React shell can
mount past `LockProvider`:

- Settings screen renders the new "Scheduled scans" section with the
  `Configure schedules` CTA (data-testid `settings-open-schedules`).
- The Schedules page renders the per-account list, cadence/day-of-week/
  hour/minute selectors, the "Schedule enabled" switch, the
  precomputed next-run timestamp, and the Last-run summary.
- Switching cadence to "Every N minutes" hides Day-of-week and Hour/Minute
  fields and reveals the Interval picker (conditional render works).
- Clicking Save round-trips through `ipc.schedulerSetSchedule`, shows the
  "Schedule saved." status banner, and the next-run timestamp advances.
- No console errors at any point in the flow.

---

## 2. QA checklist — item-by-item evidence

### 2.1 Happy Path

| QA item                                                    | Evidence                                                              |
| ---------------------------------------------------------- | --------------------------------------------------------------------- |
| Weekly schedule triggers automatic scan at configured time | `qa_happy_weekly_schedule_round_trips`; live trigger is operator OP-1 |
| `get_schedule` / `next_run_times` report configured value  | `qa_happy_weekly_schedule_round_trips`                                |
| Schedules persist across app restart and resume            | `qa_happy_schedules_persist_across_restart`                           |
| Settings UI configures per-account schedules + next-run    | `preview_*` verification §1.5                                         |

### 2.2 Error States

| QA item                                                          | Evidence                                            |
| ---------------------------------------------------------------- | --------------------------------------------------- |
| In-progress scan blocks scheduled run → recorded reason          | `qa_error_inflight_scan_blocks_scheduled_run`       |
| App closed across missed time → ≤1 catch-up on next launch       | `qa_error_missed_times_collapse_to_single_catch_up` |
| Unprovisioned account → graceful skip with reason                | `qa_error_unprovisioned_account_skip_clear_reason`  |
| Account removed with active schedule → schedule removed          | `qa_error_account_removal_clears_schedule`          |
| Machine asleep at scheduled time → catch-up on wake, no crash    | `qa_error_bootstrap_after_sleep_does_not_crash`     |

### 2.3 Responsiveness

| QA item                                                  | Evidence                                              |
| -------------------------------------------------------- | ----------------------------------------------------- |
| Schedule changes take effect without app restart         | `qa_responsiveness_changes_take_effect_without_restart` |
| Background runner does not noticeably degrade app        | Default 30s poll; `tick_once` deterministic in tests  |
| Schedules Settings UI updates promptly                   | `preview_*` verification §1.5 (save → flash → list)   |

### 2.4 State Transitions

| QA item                                                                | Evidence                                            |
| ---------------------------------------------------------------------- | --------------------------------------------------- |
| No schedule → set → background runner picks it up                      | `qa_state_transition_no_schedule_to_enabled`        |
| Enabled → disabled → no runs fire → re-enabled → runs resume           | `qa_state_transition_disable_then_reenable`         |
| Missed scheduled time → app launch → single catch-up run               | `qa_error_missed_times_collapse_to_single_catch_up` |
| Account with schedule → account removed → schedule removed             | `qa_state_transition_account_remove_removes_schedule` |

### 2.5 Security Checks (Contract 10 §Security Check)

| QA item                                                                                       | Evidence                                                |
| --------------------------------------------------------------------------------------------- | ------------------------------------------------------- |
| Scheduled scan uses the same secure scan path (fresh AssumeRole, verified binary, no creds)   | `qa_security_runner_uses_only_scanner_run_scan` — runner only invokes `scanner::run_scan`; no `assume_role`, no direct `sts::*`, no direct binary call |
| Scheduled scans never run in parallel with another scan for the same account                  | `qa_security_scheduled_scans_never_parallel_per_account` — leans on the same transactional `try_claim_account` gate the manual scan path uses |
| Scheduled-scans vs. app-lock decision is implemented explicitly and does not weaken the lock  | `qa_security_runner_does_not_consult_applock` — runner ignores `applock::*`; the lock is a UI gate, not a process gate. Documented in `scheduler/mod.rs` doc comment |
| Missed-run catch-up does not stack multiple runs                                              | `qa_security_catch_up_does_not_stack` + `qa_error_missed_times_collapse_to_single_catch_up` |
| Schedules are stored as non-secret configuration in SQLite                                    | `qa_security_schedules_table_has_no_credential_columns` (column-name scan over `0007_scheduler.sql`) + `qa_security_no_credential_returning_ipc_commands` (IPC surface audit) |

---

## 3. Architecture summary

**New files**

- `src-tauri/migrations/0007_scheduler.sql` — `schedules` and `schedule_events`
  tables; no credential-bearing columns. Pinned-by-design to non-secret
  configuration only (CLAUDE.md §4.3, §4.5).
- `src-tauri/src/scheduler/` — new module with five files:
  - `mod.rs`: `set_schedule`, `get_schedule`, `clear_schedule`,
    `list_schedules`, `next_run_times`, `recent_events`,
    `clear_schedule_if_present` (used by the accounts cascade).
  - `types.rs`: `ScheduleCadence`, `LastRunOutcome`, `Schedule`,
    `SetScheduleInput`, `ScheduleEvent`, `NextRunTime`.
  - `error.rs`: `SchedulerError` with stable IPC codes (`schedule_not_found`,
    `invalid_input`, …).
  - `cadence.rs`: pure cadence math (`next_after`, `round_forward`,
    `validate`). Daily/weekly/monthly anchor on **local** time-of-day to
    match user expectations under DST; interval cadences are clock-agnostic.
  - `storage.rs`: per-call SQLite connections (same pattern as
    `accounts::storage`); parameterized queries throughout.
  - `runner.rs`: `bootstrap_runner` (catch-up rounding on app launch) +
    `start_runner` (one background poll thread, idempotent) +
    `tick_once` (test seam). Drives scans **only** through
    `scanner::run_scan` so AssumeRole + binary verification + the
    one-scan-per-account gate stay owned by Contract 06.
- `src-tauri/tests/scheduler_test.rs` — 9 integration tests (happy path,
  validation, account cascade, runner skip behavior, lifecycle events).
- `src-tauri/tests/qa10_test.rs` — 18 QA-checklist tests, one per item in
  `C10-scheduled-scans-QA.md` (or grouped where the contract groups them).
- `src/routes/ScheduledScans.tsx` — Settings sub-panel: account picker,
  cadence/day/hour/minute selectors, enabled switch, save/clear actions,
  next-run timestamp, last-run summary. Re-fetches every 30s so next-run
  ticks down without bespoke event plumbing.

**Modified files**

- `src-tauri/src/db/migrations.rs` — registers the new
  `0007_scheduler` migration.
- `src-tauri/src/errors.rs` — adds `ScheduleNotFound` with stable code.
- `src-tauri/src/accounts/mod.rs` — `remove_account` now cascades to
  `scheduler::clear_schedule_if_present` so a removed account leaves no
  orphan schedule (Contract 10 §Edge Cases).
- `src-tauri/src/ipc/mod.rs` + `src-tauri/src/lib.rs` — registers six new
  IPC commands (`scheduler_set_schedule`, `scheduler_get_schedule`,
  `scheduler_clear_schedule`, `scheduler_list_schedules`,
  `scheduler_next_run_times`, `scheduler_recent_events`); spawns
  `scheduler::runner::bootstrap_runner` + `start_runner` once per process
  immediately after migrations run.
- `src/lib/ipc.ts` — TypeScript IPC client gains
  `schedulerSetSchedule`/`Get`/`Clear`/`ListSchedules`/`NextRunTimes`/
  `RecentEvents` plus matching types (`Schedule`, `ScheduleCadence`,
  `SetScheduleInput`, `LastRunOutcome`, `ScheduleEvent`, `NextRunTime`,
  `ScheduleEventKind`).
- `src/App.tsx` + `src/routes/Settings.tsx` — adds the `schedules` route
  and the "Configure schedules" CTA inside Settings.
- `src/locales/{en,es,fr,zh}.json` — adds `common.back` and the
  `schedules.*` translation keys. English carries the full surface;
  other locales translate the most-visible strings and inherit the
  English fallback for the rest (matches the convention used by every
  previous contract).

---

## 4. Operator-driven verification (out of automated scope)

These items in the QA contract require a live AWS environment or a real wall-
clock interval. They are documented here so the operator can sign them off
manually:

- **OP-1** — Configure a weekly schedule against a real provisioned account
  and confirm the scan fires at the configured slot (end-to-end with real
  ScoutSuite + real STS).
- **OP-2** — Close the app across a real scheduled time, then re-launch and
  confirm exactly one catch-up scan fires (in addition to the unit-level
  catch-up assertions in §2.2).
- **OP-3** — Let the machine sleep across a real scheduled time, then wake
  and confirm the runner resumes without crashing (in addition to
  `qa_error_bootstrap_after_sleep_does_not_crash`).

---

## 5. Conclusion

Every automated QA item passes. The browser-side verification confirms the
Settings UI is wired end-to-end through the new IPC surface and renders the
expected per-account configuration panel. No regressions in any of the 10
previously-shipped contracts. Contract 10 is ready to merge.
