# Contract 05 — Verification Summary

Maps each acceptance check in `C05-terraform-provisioner-QA.md` to how it was
verified. Items split into three buckets:

- **Automated** — verified by `cargo test --tests` and/or `npm run lint`.
- **Code review** — verified by inspecting the implementation; cited with
  file:line references that reviewers can re-check.
- **Operator-driven** — requires a live AWS test account and the bundled
  Terraform binary (Next Steps C2, Contract 16). These are deferred to the
  operator running the QA pass and listed at the bottom with reproduction
  steps.

Every item is accounted for. Nothing was skipped.

---

## Happy Path

| # | Check | Verification |
|---|---|---|
| 1 | `detect_terraform` reports binary present and integrity-valid | **Automated** — [`qa05_test::qa_happy_detect_terraform_succeeds_when_binary_matches_pinned_hash`](src-tauri/tests/qa05_test.rs), [`terraform_test::detect_terraform_reports_available_when_hash_matches`](src-tauri/tests/terraform_test.rs) |
| 2 | `plan` produces a readable diff | **Automated** (parser) — [`runner::tests::parse_plan_changes_extracts_address_kind_and_field_names`](src-tauri/src/terraform/runner.rs), [`qa05_test::qa_responsiveness_plan_change_shape_is_ui_renderable`](src-tauri/tests/qa05_test.rs). End-to-end: operator-driven (#OP-1). |
| 3 | `apply` against a confirmed plan creates `CloudSawScannerRole` | **Operator-driven** (#OP-2). |
| 4 | `aws iam get-role --role-name CloudSawScannerRole` confirms the role | **Operator-driven** (#OP-2). |
| 5 | UI walks detect → plan diff → confirm → apply → result | **Code review** — [`ProvisionScannerRole.tsx`](src/routes/ProvisionScannerRole.tsx); phase machine `detecting → detect_result → planning → plan_ready → applying → applied`. **Operator-driven** end-to-end via `tauri dev` (#OP-3). |

## Error States

| # | Check | Verification |
|---|---|---|
| 1 | Tampered Terraform binary → `plan`/`apply` fail with integrity error | **Automated** — [`qa05_test::qa_error_tampered_binary_yields_integrity_failed`](src-tauri/tests/qa05_test.rs), [`terraform_test::detect_terraform_reports_integrity_failed_when_binary_tampered`](src-tauri/tests/terraform_test.rs), [`terraform_test::locate_and_verify_returns_typed_errors_in_each_failure_mode`](src-tauri/tests/terraform_test.rs) |
| 2 | Stale `plan_token` → apply rejected | **Automated** — [`terraform_test::plan_token_supersession_rejects_stale_applies`](src-tauri/tests/terraform_test.rs), [`plans::tests::consume_with_stale_token_returns_expired`](src-tauri/src/terraform/plans.rs), [`qa05_test::qa_error_apply_with_invalid_token_is_rejected_before_terraform_runs`](src-tauri/tests/qa05_test.rs) |
| 3 | Insufficient IAM permissions → clear permission error; no unsafe partial state | **Code review** — `runner::terraform_apply` returns `TerraformError::ApplyFailed`; storage records the stable code via `record_failure`. No row-level state is mutated until success. **Operator-driven** end-to-end (#OP-4). |
| 4 | `apply` interrupted partway → state persists; re-running resumes idempotently | **Automated** (workdir invariant) — [`qa05_test::qa_error_workdir_sync_preserves_existing_state_files`](src-tauri/tests/qa05_test.rs). End-to-end (real Terraform): **Operator-driven** (#OP-5). |
| 5 | Bundled module source missing/corrupt → clear error, no execution | **Automated** — [`qa05_test::qa_error_missing_module_source_yields_internal_module_source_missing`](src-tauri/tests/qa05_test.rs) |

## Responsiveness

| # | Check | Verification |
|---|---|---|
| 1 | `plan` and `apply` report progress to the UI rather than appearing frozen | **Code review** — [`ProvisionScannerRole.tsx`](src/routes/ProvisionScannerRole.tsx): footer renders the busy label (`"Planning…"` / `"Applying…"`) during async operations; the primary CTA is disabled while the request is in flight; phase transitions trigger React re-renders so the user always sees the current state. |
| 2 | The plan diff renders clearly and is readable | **Automated** (data shape) — [`qa05_test::qa_responsiveness_plan_change_shape_is_ui_renderable`](src-tauri/tests/qa05_test.rs). **Code review** — `PlanSection` / `PlanChangeRow` render kind badge, resource address, and affected field names. |

## State Transitions

| # | Check | Verification |
|---|---|---|
| 1 | No role → plan → confirm → apply → role exists | **Automated** (status machine) — [`qa05_test::qa_state_transition_not_provisioned_to_provisioned`](src-tauri/tests/qa05_test.rs). End-to-end against AWS: **Operator-driven** (#OP-2). |
| 2 | Partial apply → resume → convergent complete state | **Automated** (workdir survives) — [`qa05_test::qa_error_workdir_sync_preserves_existing_state_files`](src-tauri/tests/qa05_test.rs). End-to-end: **Operator-driven** (#OP-5). |
| 3 | Role exists → plan → no destructive change → apply safe no-op | **Code review** — Terraform's `-detailed-exitcode` returns 0 for no-changes; `runner::terraform_plan` accepts code 0 as a valid plan with `no_changes = true`; `PlanResult.no_changes` drives the "safe no-op" UI copy. **Operator-driven** end-to-end (#OP-6). |

## Security Check

| # | Check | Verification |
|---|---|---|
| 1 | Terraform invoked by absolute path with argv arrays; no shell | **Automated** — [`qa05_test::qa_security_runner_source_has_no_shell_invocation`](src-tauri/tests/qa05_test.rs). **Code review** — [`runner.rs`](src-tauri/src/terraform/runner.rs) uses `Command::new(&tf).args(args)` with the verified absolute path; `cmd.stdin(Stdio::null())` and `TF_INPUT=0` prevent interactive prompts. |
| 2 | Binary SHA-256 is verified against the build-pinned hash before every execution; tampered binary rejected | **Automated** — [`qa05_test::qa_security_run_path_invokes_locate_and_verify`](src-tauri/tests/qa05_test.rs), [`qa05_test::qa_error_tampered_binary_yields_integrity_failed`](src-tauri/tests/qa05_test.rs), [`binary::tests::verify_sha256_rejects_mismatched_hash`](src-tauri/src/terraform/binary.rs). **Code review** — every `run()` call routes through `prepare_invocation()` → `binary::locate_and_verify()`. |
| 3 | Created role's trust policy principal equals the live caller ARN; never a wildcard | **Automated** — [`qa05_test::qa_security_trust_policy_verifier_rejects_wildcard_and_mismatch`](src-tauri/tests/qa05_test.rs), [`runner::tests::verify_trust_policy_rejects_wildcard_principal`](src-tauri/src/terraform/runner.rs). The Terraform module also blocks `*` at the `variables.tf` validator layer (defense in depth). |
| 4 | Federated/assumed-role caller resolves to the underlying role ARN | **Automated** — [`qa05_test::qa_security_assumed_role_session_unwraps_to_underlying_role_arn`](src-tauri/tests/qa05_test.rs), [`identity::tests::assumed_role_unwraps_to_iam_role_arn`](src-tauri/src/terraform/identity.rs), [`identity::tests::sso_assumed_role_uses_aws_reserved_path`](src-tauri/src/terraform/identity.rs) |
| 5 | Default attached policy is `SecurityAudit`; `ReadOnlyAccess` requires explicit opt-in with visible warning | **Automated** — [`qa05_test::qa_security_default_policy_variant_is_security_audit`](src-tauri/tests/qa05_test.rs), [`qa05_test::qa_security_read_only_access_warning_present_in_en_locale`](src-tauri/tests/qa05_test.rs). The check covers Rust default, Terraform module default, React state default, and the en.json warning copy. |
| 6 | No `terraform destroy` command is exposed anywhere | **Automated** — [`qa05_test::qa_security_no_terraform_destroy_command_exposed`](src-tauri/tests/qa05_test.rs) scans `ipc/mod.rs`, `lib.rs`, and `runner.rs` for any "destroy" reference. |
| 7 | Terraform state lives only in `tf-work/{account_id}/` with user-only permissions; none in bundle or repo | **Automated** — [`qa05_test::qa_security_workdir_lives_under_app_data_dir_and_account_segment`](src-tauri/tests/qa05_test.rs), [`qa05_test::qa_security_no_terraform_state_in_repo`](src-tauri/tests/qa05_test.rs), [`terraform_test::workdir_prepare_copies_module_files_and_stays_under_data_dir`](src-tauri/tests/terraform_test.rs). `workdir::prepare` calls `ensure_user_only_dir`; `tfvars` writes call `set_user_only`. |

---

## Operator-driven checks (live AWS required)

The following items require an AWS test account and the bundled Terraform
binary (the latter pinned by Contract 16 / Next-Steps C2 and not yet present
in the dev build). The Rust unit tests use a placeholder `PLATFORM_PINNED_SHA256
= None`, so `detect_terraform` reports `Missing` in the absence of a real
binary — that is the contract's "no binary bundled" edge case behaving
correctly. To exercise these checks, install a release build (or hand-drop a
binary into `src-tauri/vendor/terraform/<triple>/terraform[.exe]` and set the
`CLOUDSAW_TERRAFORM_SHA256_OVERRIDE` env var) and follow the steps below.

**#OP-1 — `plan` against a real account produces a readable diff**
1. Add a CloudSaw account pointing at a test AWS profile (Accounts → Add).
2. Click **Provision scanner role** on that row.
3. Accept the SecurityAudit default and click **Generate plan**.
4. **Expect:** the modal shows a Plan Diff section with two `Create` rows
   (`aws_iam_role.scanner`, `aws_iam_role_policy_attachment.scanner`), each
   listing affected field names but no values.

**#OP-2 — `apply` creates `CloudSawScannerRole`, confirmed via `aws iam get-role`**
1. Complete #OP-1 to reach an applyable plan.
2. Click **Apply plan**.
3. **Expect:** the Applied section shows the role ARN
   (`arn:aws:iam::<acct>:role/CloudSawScannerRole`) and a trust-policy
   SHA-256.
4. From a terminal: `aws --profile <profile> iam get-role --role-name CloudSawScannerRole`
5. **Expect:** the role exists; `Role.AssumeRolePolicyDocument.Statement[0].Principal.AWS`
   equals the caller's underlying role ARN (or IAM user ARN); the attached
   policy is `arn:aws:iam::aws:policy/SecurityAudit`.

**#OP-3 — UI flow end-to-end**
Verified incidentally by running #OP-1 and #OP-2 through the modal.

**#OP-4 — Insufficient IAM permissions surfaces a clear, redacted error**
1. Use a test profile that lacks `iam:CreateRole`.
2. Run the provisioning flow.
3. **Expect:** the modal shows `terraform_apply_failed` mapped to
   "terraform apply failed. The AWS credentials may lack the IAM
   permissions needed to read role state." No raw AWS SDK output appears.
4. **Expect:** the accounts row remains `Not provisioned`; the `accounts`
   table's `last_provisioning_error` column equals `"terraform_apply_failed"`.

**#OP-5 — Interrupted apply resumes idempotently**
1. Run #OP-1 and click **Apply plan**.
2. While `terraform apply` is in flight, kill the CloudSaw process.
3. Restart CloudSaw. The `accounts` row may show "Last attempt failed".
4. Click **Re-plan scanner role** on that row and Apply.
5. **Expect:** Terraform re-acquires its state from `tf-work/<acct>/`, plans
   only the remaining changes, and applies them. Re-running again is a no-op.

**#OP-6 — Re-planning a provisioned account is a no-op**
1. Complete #OP-2 successfully.
2. Click **Re-plan scanner role** on the same row.
3. **Expect:** the plan modal shows
   "No changes. The scanner role already matches the desired state — applying
   is a safe no-op."
4. Apply anyway.
5. **Expect:** no destructive changes; `Provisioned` badge unchanged.

---

## How to reproduce the automated checks

```sh
# Rust suite (49 unit + 5 integration files):
cd src-tauri
cargo test --lib
cargo test --test accounts_test
cargo test --test applock_test
cargo test --test auth_test
cargo test --test migrations_test
cargo test --test terraform_test
cargo test --test qa05_test

# TypeScript / Vite:
cd ..
npm run lint
```

All Rust suites finish green; `npm run lint` (tsc --noEmit) reports zero
errors.

## Open items deferred to later contracts

- **Per-target Terraform binary + SHA-256 manifest** — set up by Contract 16
  (Release Pipeline) using the hash table laid out in
  `src-tauri/src/terraform/binary.rs` (`PLATFORM_PINNED_SHA256`). Until then,
  `detect_terraform` reports `Missing` on dev builds.
- **`sts:AssumeRole` against `CloudSawScannerRole`** — Contract 06 (Scanner
  Orchestrator) uses the persisted `scanner_role_arn` + `external_id`.
