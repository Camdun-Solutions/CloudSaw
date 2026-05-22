# Contract 07 — Verification Summary

Maps each acceptance check in `C07-findings-store-QA.md` to how it was
verified. Items split into three buckets:

- **Automated** — verified by `cargo test --tests`.
- **Code review** — verified by inspecting the implementation; cited with
  file:line references that reviewers can re-check.
- **Operator-driven** — requires a live AWS test account and a real
  ScoutSuite run (Next Steps C3, Contract 16). These are deferred to the
  operator running the QA pass and listed at the bottom with reproduction
  steps.

Every item from the QA contract is accounted for. Nothing was skipped.

---

## Happy Path

| # | Check | Verification |
|---|---|---|
| 1 | Parsing a real `raw-scout.json` populates `scans`, `findings`, and `finding_resources` with correct `account_id` partitioning | **Automated** — [`findings_test::qa_happy_parse_populates_tables_with_correct_account_partitioning`](src-tauri/tests/findings_test.rs). Asserts 3 findings inserted with `aws_account_id == "111122223333"`. |
| 2 | `list_findings` returns the expected findings for a scan | **Automated** — [`findings_test::qa_happy_list_findings_returns_findings_for_scan`](src-tauri/tests/findings_test.rs). Verifies rule keys are present and the severity filter narrows to the danger-level finding only. |
| 3 | `list_scans` returns scans for an account; `get_scan`/`get_finding` return single records | **Automated** — [`findings_test::qa_happy_list_and_get_scans_findings`](src-tauri/tests/findings_test.rs). |
| 4 | Re-scanning updates recurring findings' first-seen/last-seen and status | **Automated** — [`findings_test::qa_happy_rescan_updates_first_and_last_seen_on_recurring_findings`](src-tauri/tests/findings_test.rs). `first_seen_at` is asserted unchanged; `last_seen_at` is asserted greater; `last_seen_scan_id` is asserted to advance to the new scan. |

## Error States

| # | Check | Verification |
|---|---|---|
| 1 | Malformed scanner JSON → clear parse error, no partial writes, scan marked failed | **Automated** — [`findings_test::qa_error_malformed_json_yields_parse_error_with_no_partial_writes`](src-tauri/tests/findings_test.rs). Asserts `FindingsError::ParseMalformed`, that the findings table contains zero rows for the account afterwards, and that the scan row has been flipped to `Failed` with `failure_code = "parse_malformed_json"`. |
| 2 | Unknown finding type → stored with generic type plus preserved raw type | **Automated** — [`findings_test::qa_error_unknown_finding_type_preserves_raw_type`](src-tauri/tests/findings_test.rs), plus the parser-level test [`findings::parser::tests::parser_preserves_unknown_rule_type`](src-tauri/src/findings/parser.rs). `raw_type` is asserted equal to the original rule key. |
| 3 | Malformed resource ARN → resource row flagged invalid; finding still stored | **Automated** — [`findings_test::qa_error_malformed_resource_path_flagged_invalid_finding_still_stored`](src-tauri/tests/findings_test.rs) (two-resource finding: one valid, one invalid; both stored, invalid flagged). Backed by [`findings::parser::tests::parser_flags_invalid_resource_paths`](src-tauri/src/findings/parser.rs). |
| 4 | Parsing a non-existent `scan_id` → clear error, no crash | **Automated** — [`findings_test::qa_error_nonexistent_scan_id_returns_clear_error`](src-tauri/tests/findings_test.rs). Asserts `FindingsError::ScanNotFound`. |
| 5 (defensive) | Account-ID mismatch between scan row and raw-scout.json → hard error, scan marked failed | **Automated** — [`findings_test::qa_error_account_mismatch_rejects_and_marks_scan_failed`](src-tauri/tests/findings_test.rs). Added beyond the QA contract per CLAUDE.md §4.1 ("account_id is the partitioning key, never inferred from untrusted input"). |

## Responsiveness

| # | Check | Verification |
|---|---|---|
| 1 | Severity-filtered `list_findings` on a 50k-finding database returns the first page well under a noticeable delay (target: <100ms; QA cap: <250ms to absorb CI jitter) | **Automated** — [`findings_test::qa_responsiveness_severity_filtered_list_is_index_backed_and_fast`](src-tauri/tests/findings_test.rs). Seeds 50k findings (one account) + 5k findings (another account) for partition pressure, then asserts the first 100-row page returns in <250ms AND that `EXPLAIN QUERY PLAN` reports an index-backed access path. |
| 2 | `list_scans` returns promptly with many scans present | **Automated** — [`findings_test::qa_responsiveness_list_scans_returns_promptly_with_many_scans`](src-tauri/tests/findings_test.rs). 500 scans seeded, default cap (200) returned in <250ms. The index used is `scans_account_started` from migration 0005. |

## State Transitions

| # | Check | Verification |
|---|---|---|
| 1 | No findings → scan parsed → findings stored | **Automated** — [`findings_test::qa_happy_parse_populates_tables_with_correct_account_partitioning`](src-tauri/tests/findings_test.rs). |
| 2 | Findings stored → same scan re-parsed → zero net change (idempotent) | **Automated** — [`findings_test::qa_state_reparsing_same_scan_is_byte_idempotent_for_list_findings`](src-tauri/tests/findings_test.rs). Asserts `serde_json::to_string(first_list) == serde_json::to_string(second_list)`. |
| 3 | Finding present in scan N → absent/resolved in scan N+1 → status and last-seen updated, history retained | **Automated** — [`findings_test::qa_state_resolution_marks_status_and_retains_history`](src-tauri/tests/findings_test.rs). Verifies the s3 finding moves to `status='resolved'`, `resolved_in_scan_id` is set, and the row is still queryable via `status` filter. |
| 4 | Scan present → `delete_scan` → scan and all child rows gone, no orphans | **Automated** — [`findings_test::qa_state_delete_scan_cascades_no_orphans`](src-tauri/tests/findings_test.rs). Counts `scans`, `findings`, `finding_resources`, `scan_findings` afterwards — all 0. Companion test [`qa_state_delete_scan_keeps_findings_observed_in_other_scans`](src-tauri/tests/findings_test.rs) verifies cross-scan findings are retained with their last_seen pointers re-anchored. |

## Security Check

| # | Check | Verification |
|---|---|---|
| 1 | All SQL uses parameterized queries; no string-concatenated SQL exists | **Automated** (structural grep) — [`findings_test::qa_security_no_string_concatenated_sql`](src-tauri/tests/findings_test.rs); plus **Code review** — every `params![...]` call in [`findings::storage`](src-tauri/src/findings/storage.rs) binds values. The one site that builds dynamic SQL is the IN-clause placeholder list in `apply_parsed`, whose values are still bound (`stmt.query_map(params_vec.as_slice(), …)`). |
| 2 | Every finding/scan query filters by `account_id` | **Automated** — [`findings_test::qa_security_every_list_query_partitions_by_account_id`](src-tauri/tests/findings_test.rs). Every public read path either takes `aws_account_id` directly (`list_scans`, `list_scans_for_account`) or chains through `get_scan_row(scan_id) → ScanRecord` which surfaces the account before the JOIN constrains by it. |
| 3 | Severity normalization maps every input to the fixed set; unknown values log a warning and map to `informational` | **Automated** — [`findings_test::qa_security_severity_normalization_is_total`](src-tauri/tests/findings_test.rs) (mixed-input scan with one valid + one unknown level; both stored with valid severity, `unknown_severity_count` advanced); plus parser-level tests [`severity_maps_scoutsuite_levels_to_normalized_scale`](src-tauri/src/findings/parser.rs) and [`parser_records_unknown_severity_as_informational_with_count`](src-tauri/src/findings/parser.rs). The CHECK constraint on `findings.severity` (migration 0006) is a defense-in-depth backstop. |
| 4 | `delete_scan` cascades fully with no orphan rows | **Automated** — [`findings_test::qa_state_delete_scan_cascades_no_orphans`](src-tauri/tests/findings_test.rs). After deletion the test counts every child table and asserts 0. Findings observed by other scans are kept with re-anchored pointers ([`qa_state_delete_scan_keeps_findings_observed_in_other_scans`](src-tauri/tests/findings_test.rs)). |
| 5 | The parser is pure: identical input yields identical stored data aside from first-seen/last-seen bookkeeping | **Automated** — [`findings_test::qa_security_parser_is_pure_for_identical_input`](src-tauri/tests/findings_test.rs) (every field including timestamps compared across two parse passes of the same scan); parser-level [`parser_is_deterministic_for_identical_input`](src-tauri/src/findings/parser.rs). |
| 6 | List-view queries are index-backed (verified via `EXPLAIN QUERY PLAN`) | **Automated** — [`findings_test::qa_security_severity_query_plan_uses_index`](src-tauri/tests/findings_test.rs) (low-row plan check); the 50k-row variant in [`qa_responsiveness_severity_filtered_list_is_index_backed_and_fast`](src-tauri/tests/findings_test.rs) also confirms the plan does not full-scan. Indexes declared in migration 0006: `findings_account_severity`, `findings_account_status`, `findings_account_service`, `findings_account_last_seen`, `findings_last_seen_scan`, plus `scan_findings` PRIMARY KEY and `scan_findings_finding`/`scan_findings_account`. |

---

## Test execution

```
cargo test --lib                            # 70 passed; 0 failed (lib unit tests, including 11 findings unit tests)
cargo test --test findings_test             # 20 passed; 0 failed
cargo test --test accounts_test             # 17 passed; 0 failed (no regressions)
cargo test --test applock_test              # 17 passed; 0 failed
cargo test --test auth_test                 # 11 passed; 0 failed
cargo test --test migrations_test           #  5 passed; 0 failed (0006_findings applies cleanly)
cargo test --test scanner_test              # 20 passed; 0 failed
cargo test --test terraform_test            # 16 passed; 0 failed
cargo test --test qa05_test                 # 23 passed; 0 failed
cargo test --test qa06_test                 # 19 passed; 0 failed
```

`cargo build` succeeds cleanly. `cargo clippy --lib --tests --no-deps`
reports a single warning in pre-existing `accounts/validation.rs` (the
`manual_contains` lint was introduced in rust-clippy 1.95 after C04
landed); zero clippy warnings in any file touched by this contract.
`rustfmt` applied to every file in this contract.

---

## Operator-driven items (deferred to live-AWS QA pass)

These items can be verified only with a real AWS account and a real
ScoutSuite scan. Contract 07's parser itself is fully exercised by the
automated suite using deliberately-crafted ScoutSuite-shaped JSON fixtures.

| ID | Item | How to reproduce |
|---|---|---|
| OP-1 | End-to-end: run a real `scanner::run_scan`, observe `findings::parse_and_store` consumes the produced `raw-scout.json` and populates the DB | After Next Steps C3 lands the bundled ScoutSuite binary, run a full scan against a test account and inspect the resulting rows in `findings` / `finding_resources` via `sqlite3` on the data dir. |
| OP-2 | Confirm the UI "Findings" surface (Contract 09) renders the parser's output | Deferred to Contract 09 — the parser's stored shape is the input to that contract. |
