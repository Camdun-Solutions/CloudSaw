# Contract 06 — Verification Summary

Maps each acceptance check in `C06-scanner-orchestrator-QA.md` to how it was
verified. Items split into three buckets:

- **Automated** — verified by `cargo test --tests` and/or `npm run lint`.
- **Code review** — verified by inspecting the implementation; cited with
  file:line references that reviewers can re-check.
- **Operator-driven** — requires a live AWS test account and the bundled
  ScoutSuite binary (Next Steps C3, Contract 16). These are deferred to the
  operator running the QA pass and listed at the bottom with reproduction
  steps.

Every item is accounted for. Nothing was skipped.

---

## Happy Path

| # | Check | Verification |
|---|---|---|
| 1 | A scan against a provisioned account runs end to end to `complete` (or `complete_with_warnings`) | **Automated** — [`scanner_test::run_scan_walks_to_complete_in_dry_run_mode`](src-tauri/tests/scanner_test.rs), [`qa06_test::qa_happy_scan_completes_and_produces_non_empty_raw_output`](src-tauri/tests/qa06_test.rs). End-to-end against AWS: **Operator-driven** (#OP-1). |
| 2 | `raw-scout.json` is produced and non-empty | **Automated** (dry-run handoff file) — [`scanner_test::run_scan_walks_to_complete_in_dry_run_mode`](src-tauri/tests/scanner_test.rs). End-to-end with real ScoutSuite: **Operator-driven** (#OP-1). |
| 3 | `scan_status` reflects the correct status transitions throughout | **Automated** — [`qa06_test::qa_state_transition_pending_to_complete_walks_each_state`](src-tauri/tests/qa06_test.rs); the orchestrator transitions through `assuming_role → scanning → parsing → complete` in [`scanner/mod.rs::execute_scan_inner`](src-tauri/src/scanner/mod.rs). |
| 4 | The UI starts a scan for the active account and shows progress | **Code review** — [`ScanProgress.tsx`](src/routes/ScanProgress.tsx); phase machine `detecting → detect_result → starting → running → terminal`; "Scan now" button wired in [`Accounts.tsx`](src/routes/Accounts.tsx). **Operator-driven** end-to-end via `tauri dev` (#OP-2). |

## Error States

| # | Check | Verification |
|---|---|---|
| 1 | Tampered ScoutSuite binary → scan does not start; integrity error | **Automated** — [`qa06_test::qa_error_tampered_binary_yields_integrity_failed`](src-tauri/tests/qa06_test.rs), [`scanner_test::run_scan_rejects_when_binary_integrity_fails`](src-tauri/tests/scanner_test.rs), [`scanner::binary::tests::verify_sha256_rejects_mismatched_hash`](src-tauri/src/scanner/binary.rs) |
| 2 | AssumeRole failure → scan fails early with a clear reason; no child spawned | **Automated** (early gate) — [`qa06_test::qa_error_assume_role_failure_fails_early_without_spawning`](src-tauri/tests/qa06_test.rs). Real STS classification: [`scanner::sts::classify`](src-tauri/src/scanner/sts.rs). End-to-end against AWS: **Operator-driven** (#OP-3). |
| 3 | Second concurrent scan for the same account → rejected | **Automated** — [`scanner_test::second_concurrent_scan_for_same_account_is_rejected`](src-tauri/tests/scanner_test.rs), [`qa06_test::qa_error_second_concurrent_scan_rejected`](src-tauri/tests/qa06_test.rs); the transactional rejection lives in [`scanner::storage::try_claim_account`](src-tauri/src/scanner/storage.rs). |
| 4 | Machine sleep loses the child → scan marked `failed` (process-lost) on resume | **Automated** — [`qa06_test::qa_error_machine_sleep_marks_scan_process_lost_on_resume`](src-tauri/tests/qa06_test.rs), [`scanner_test::reap_stale_on_boot_marks_in_flight_rows_failed`](src-tauri/tests/scanner_test.rs); [`scanner::reap_stale_on_boot`](src-tauri/src/scanner/mod.rs) runs at app bootstrap. |
| 5 | Role missing permissions → `complete_with_warnings` with missing-permission detail | **Automated** (storage round-trip) — [`qa06_test::qa_error_partial_permissions_yield_complete_with_warnings`](src-tauri/tests/qa06_test.rs); orchestrator maps ScoutSuite exit code `2` onto warnings in [`runner::classify_exit`](src-tauri/src/scanner/runner.rs) → [`execute_scan_inner`](src-tauri/src/scanner/mod.rs). End-to-end: **Operator-driven** (#OP-4). |
| 6 | Extremely large scanner output → bounded/truncated with a warning; raw file retained | **Automated** (round-trip + bounded reader) — [`qa06_test::qa_error_truncated_output_flagged_in_scan_record`](src-tauri/tests/qa06_test.rs); the bounded-reader implementation lives in [`runner::spawn_bounded_reader`](src-tauri/src/scanner/runner.rs) (cap `STREAM_CAP_BYTES = 4 MiB`). |

## Responsiveness

| # | Check | Verification |
|---|---|---|
| 1 | `scan_status` polling returns promptly and reflects current state | **Automated** — [`qa06_test::qa_responsiveness_scan_status_returns_promptly`](src-tauri/tests/qa06_test.rs) (asserts < 250ms round-trip). |
| 2 | Cancellation terminates the ScoutSuite process promptly | **Code review** — [`runner::wait_for_child`](src-tauri/src/scanner/runner.rs) polls the cancel flag between every `try_wait`; `handles::signal_cancel` calls `Child::kill()` immediately. End-to-end with real ScoutSuite: **Operator-driven** (#OP-5). |
| 3 | UI shows progress without freezing during a long scan | **Code review** — [`ScanProgress.tsx`](src/routes/ScanProgress.tsx) uses `setTimeout` polling at 1s intervals; the running phase keeps the modal interactive and exposes "Continue in background" / "Cancel scan" actions. **Operator-driven** end-to-end (#OP-5). |

## State Transitions

| # | Check | Verification |
|---|---|---|
| 1 | `pending → assuming_role → scanning → parsing → complete` | **Automated** — [`qa06_test::qa_state_transition_pending_to_complete_walks_each_state`](src-tauri/tests/qa06_test.rs); transitions live in [`scanner/mod.rs::execute_scan_inner`](src-tauri/src/scanner/mod.rs). |
| 2 | `... → complete_with_warnings` when permissions are partial | **Automated** — [`qa06_test::qa_error_partial_permissions_yield_complete_with_warnings`](src-tauri/tests/qa06_test.rs); the `Some(("missing_permissions", _))` branch in `record_complete` flips the status to `CompleteWithWarnings`. |
| 3 | `scanning → canceled` on cancel, with partial output flagged | **Automated** — [`qa06_test::qa_state_transition_scanning_to_canceled`](src-tauri/tests/qa06_test.rs), [`scanner_test::cancel_scan_transitions_to_canceled_and_is_idempotent`](src-tauri/tests/scanner_test.rs). |
| 4 | `scanning → failed` on process loss | **Automated** — [`qa06_test::qa_state_transition_scanning_to_failed_on_process_loss`](src-tauri/tests/qa06_test.rs); covered by `reap_stale_on_boot` calling `record_failed("scanner_process_lost")`. |

## Security Check

| # | Check | Verification |
|---|---|---|
| 1 | Each scan performs a fresh `AssumeRole`; no STS credentials persist across scans | **Automated** (structural) — [`qa06_test::qa_security_assume_role_is_fresh_per_scan_no_cache`](src-tauri/tests/qa06_test.rs); the orchestrator calls `sts::assume_scanner_role` inside `execute_scan_inner` on every scan; no `static`/`OnceLock`/cache holds credentials. |
| 2 | AssumeRole session duration ≤ 3600 seconds | **Automated** — [`qa06_test::qa_security_assume_role_session_duration_is_bounded`](src-tauri/tests/qa06_test.rs); pinned by `sts::SCAN_SESSION_DURATION_SECONDS = 3600`. |
| 3 | ScoutSuite binary SHA-256 is verified before every invocation; tampered binary rejected | **Automated** — [`qa06_test::qa_security_run_path_invokes_locate_and_verify`](src-tauri/tests/qa06_test.rs), [`qa06_test::qa_error_tampered_binary_yields_integrity_failed`](src-tauri/tests/qa06_test.rs), [`scanner::binary::tests::verify_sha256_rejects_mismatched_hash`](src-tauri/src/scanner/binary.rs). |
| 4 | ScoutSuite invoked by absolute path with argv arrays; no shell | **Automated** — [`qa06_test::qa_security_runner_source_has_no_shell_invocation`](src-tauri/tests/qa06_test.rs). The runner uses `Command::new(&binary_path).args(...)` with the verified absolute path; `cmd.stdin(Stdio::null())` blocks interactive prompts. |
| 5 | Temporary credentials reach only the ScoutSuite child environment — not disk, not logs, not the parent process | **Automated** (structural + functional) — [`qa06_test::qa_security_credentials_go_only_to_child_environment`](src-tauri/tests/qa06_test.rs), [`qa06_test::qa_security_credentials_never_written_to_disk`](src-tauri/tests/qa06_test.rs), [`qa06_test::qa_security_no_credentials_on_disk_after_scan`](src-tauri/tests/qa06_test.rs); [`scanner::sts::tests::debug_output_does_not_leak_secret_bytes`](src-tauri/src/scanner/sts.rs) confirms the Debug impl redacts every field. End-to-end with `ps` inspection: **Operator-driven** (#OP-6). |
| 6 | After a scan, no AWS credentials exist on disk under the app data directory | **Automated** — [`qa06_test::qa_security_no_credentials_on_disk_after_scan`](src-tauri/tests/qa06_test.rs); walks the entire data root and grep-scans every file for the stub credential bytes. |
| 7 | The scan output directory has user-only permissions | **Automated** — [`qa06_test::qa_security_scan_output_dir_user_only`](src-tauri/tests/qa06_test.rs), [`scanner_test::run_scan_persists_under_scans_dir_with_user_only_permissions`](src-tauri/tests/scanner_test.rs); [`db::paths::ensure_user_only_dir`](src-tauri/src/db/paths.rs) sets `0700` on Unix. |

---

## Operator-driven checks (live AWS + bundled ScoutSuite required)

The following items require an AWS test account with a provisioned
`CloudSawScannerRole` (Contract 05 #OP-2) and the bundled ScoutSuite binary
(pinned by Contract 16 / Next-Steps C3 and not yet present in the dev build).
Until then, `detect_binary` reports `Missing` on dev builds. To exercise these
checks, install a release build (or hand-drop a binary into
`src-tauri/vendor/scoutsuite/<triple>/scoutsuite[.exe]` and set the
`CLOUDSAW_SCOUTSUITE_SHA256_OVERRIDE` env var) and follow the steps below.

**#OP-1 — End-to-end scan against a real account produces non-empty findings**
1. Provision the scanner role via #OP-2 from Contract 05.
2. From the Accounts page, click **Scan now** on the provisioned row.
3. Click **Start scan**.
4. **Expect:** the modal walks `Assuming role → Scanning → Parsing → Complete`.
5. **Expect:** `scans/<scan-id>/raw-scout.json` exists in the data dir with
   non-empty contents (open it from the Reveal button or `cat` directly).

**#OP-2 — UI flow end-to-end** Verified incidentally by running #OP-1 through
the modal. The "Continue in background" button must dismiss the modal without
canceling the scan; reopening the modal must show the in-flight progress.

**#OP-3 — AssumeRole failure surfaces a clear error**
1. Manually break the role's trust policy (remove the ExternalId condition,
   or delete the role outright).
2. Run a scan.
3. **Expect:** the modal shows the `scanner_assume_role_failed` mapped to
   "CloudSaw couldn't assume the scanner role…". No raw AWS SDK output.
4. **Expect:** the accounts row's `last_scan_status` is `failure`.

**#OP-4 — Partial permissions yield `complete_with_warnings`**
1. Apply a SCP or boundary policy that denies a couple of `*:List*` actions
   the scanner uses (e.g. `s3:ListAllMyBuckets` denied).
2. Run a scan.
3. **Expect:** the modal lands on **Completed with warnings**; the warning
   row reads "missing_permissions"; `raw-scout.json` is still produced and
   non-empty.

**#OP-5 — Cancel kills the ScoutSuite process promptly**
1. Start a scan.
2. While it's in `Scanning`, click **Cancel scan**.
3. **Expect:** the modal lands on **Canceled** within ~1s.
4. From a terminal: `ps -ef | grep scoutsuite` returns no live process.

**#OP-6 — Credentials never appear in the parent process or on disk**
1. Start a scan and let it enter `Scanning`.
2. From a terminal: `ps eww <CloudSaw PID>` — the CloudSaw parent process
   environment must NOT contain `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`,
   or `AWS_SESSION_TOKEN`.
3. From the same terminal: `ps eww <scoutsuite PID>` — the child MUST
   contain those env vars.
4. After the scan completes: `grep -r "ASIA" <data_dir>` (or the actual
   access-key prefix you observed) returns no matches.

---

## How to reproduce the automated checks

```sh
# Rust suite (lib + 6 integration files):
cd src-tauri
cargo test --lib
cargo test --test accounts_test
cargo test --test applock_test
cargo test --test auth_test
cargo test --test migrations_test
cargo test --test terraform_test
cargo test --test qa05_test
cargo test --test scanner_test
cargo test --test qa06_test

# TypeScript / Vite:
cd ..
npm run lint
```

All Rust suites finish green; `npm run lint` (`tsc --noEmit`) reports zero
errors.

## Open items deferred to later contracts

- **Per-target ScoutSuite binary + SHA-256 manifest** — set up by Contract 16
  (Release Pipeline) using the hash table laid out in
  [`src-tauri/src/scanner/binary.rs`](src-tauri/src/scanner/binary.rs)
  (`PLATFORM_PINNED_SHA256`). Until then, `detect_binary` reports `Missing` on
  dev builds.
- **Findings parser (Contract 07)** — consumes the `raw-scout.json` files
  Contract 06 produces. The scanner orchestrator's handoff is just the file
  path; the parser walks it and writes structured findings to SQLite.
- **Scheduled scans (Contract 10)** — wraps `scanner::run_scan` in a
  background scheduler. Contract 06 exposes the surface; Contract 10 owns
  the cron-like trigger.
