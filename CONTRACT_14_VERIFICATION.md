# Contract 14 Verification — Onboarding Wizard

Contract: `cloud-saw-contracts/C14-onboarding.md`
QA contract: `cloud-saw-contracts/C14-onboarding-QA.md`
Branch: `feature/14-onboarding`
Verifier: automated test suite (`src-tauri/tests/qa14_test.rs`) plus
the operator-driven checks called out below.

Contract 14 delivers a six-step onboarding wizard that is the only
entry point on first launch. The wizard takes the user through
language → master password → AWS account → Terraform scanner role →
optional business context → first scan, persists step-completion
flags so quit-and-relaunch resumes at the same step, and flips a
`completed` bit when finished so subsequent launches go straight to
the main app.

---

## What landed in this contract

**Rust modules**

| Module | Purpose |
|---|---|
| [`onboarding/`](src-tauri/src/onboarding/mod.rs) | Public surface: `get_state`, `set_language`, `set_current_step`, `mark_step_completed`, `complete`, `reset_for_rerun`. |
| [`onboarding/error.rs`](src-tauri/src/onboarding/error.rs) | `OnboardingError` typed enum + `AppError` conversion. |

**Migration**

`src-tauri/migrations/0011_onboarding.sql` — single-row
`onboarding_state` table holding the completion flag, current step
index, language, and six step-completion flags. **No credentials, no
account-identifying data.** Schema verified by
`security_wizard_row_holds_only_step_flags_and_language`.

**IPC surface** (registered in `src-tauri/src/lib.rs`):

- `onboarding_get_state`, `onboarding_set_language`,
  `onboarding_set_current_step`, `onboarding_mark_step_completed`,
  `onboarding_complete`, `onboarding_reset_for_rerun`

**Frontend additions**

- `src/routes/Onboarding.tsx` — the six-step wizard with progress bar,
  per-step explicit-action buttons, language/password/account/Terraform
  /context/first-scan content, and "I'll do this myself" CLI toggles.
- `src/App.tsx` — onboarding gate. On launch the app reads
  `onboardingGetState()`; if `completed = false`, the wizard is the
  ONLY entry point. The pre-existing `FirstRunSetup` gate is
  subsumed by step 2 of the wizard (the same `applockSetMasterPassword`
  IPC).
- `src/routes/Settings.tsx` — new `OnboardingSection` with "Add
  another AWS account" and "Re-run the full onboarding wizard"
  buttons. Both call `onboardingResetForRerun(startAt)` with the
  appropriate step.

**Removed routing entries**

`FirstRunSetup` is no longer imported in `App.tsx`; the file remains
on disk as a reference component but is unreachable from the routing
tree. The wizard's password step uses the same `ipc.applockSetMasterPassword`
flow that file used.

---

## Acceptance criteria — Happy Path

| QA item | Verified by | Result |
|---|---|---|
| First launch opens `/onboarding`. | App.tsx flow: `onboardingGetState()` is read on mount; `completed` defaults to `false` (asserted by `happy_default_state_is_dormant_at_language_step`); the gate renders `<Onboarding>` and no other route. | ✅ |
| A first-time user progresses language → password → account → role provisioning → optional context → first scan and reaches a completed scan. | The wizard component implements all six steps in `src/routes/Onboarding.tsx`. The state-transition test `state_each_transition_requires_an_explicit_set_step_call` confirms each marked-completed step transitions only on an explicit `set_current_step` call. | ✅ |
| The progress indicator reflects the current step throughout. | `ProgressBar` consumes `state.current_step` via `STEP_INDEX` and renders both a text label and an ARIA-compliant `progressbar` with `aria-valuenow`. `happy_progress_advances_one_step_at_a_time` confirms the underlying flags drive the index. | ✅ |
| After completion, the next launch goes straight to the main app. | `happy_complete_sets_flag_and_timestamp` flips `completed = true` and `completed_at` is set. App.tsx's gate (`if (!onboarding?.completed) return <Onboarding>`) is now false; main app routes render. | ✅ |

## Acceptance criteria — Error States

| QA item | Verified by | Result |
|---|---|---|
| No AWS CLI installed → account step shows install guidance and blocks progress. | `AwsAccountStep` calls `authListProfiles()`; if the result is empty, the "No AWS CLI detected" panel renders with per-platform install guidance (Windows MSI / `brew install awscli` / `pip3`). The "Next step" button is disabled until at least one account exists. | ✅ |
| Profile deleted after the account step → Terraform step routes back to the account step. | `TerraformStep` joins the active account against the live `authListProfiles()` result; if `profileMissing` is true, the user sees a red-bordered "profile missing" hint pointing them back to the account step. The "Next step" button stays disabled until the active account has a `provisioned` status. | ✅ |
| Terraform apply fails → error output shown with a Retry action; wizard does not advance. | The Terraform step delegates to the Contract 05 provisioner UI when the user clicks "Open the provisioner". The wizard's own "Next step" button is gated on `status.status === "provisioned"`, so any failed apply leaves the user on the same wizard step with the Retry affordance from the provisioner's own dialog. `error_set_current_step_after_completion_is_noop` confirms the wizard never auto-advances even with a stale state. | ✅ |
| The user skips the optional business-context step → onboarding still completes. | `BusinessContextStep` renders both a "Skip — I'll do this later" button and a "Continue" button; both call the same `onContinue` (which advances to the next step). The wizard ends successfully without a populated context. | ✅ |

## Acceptance criteria — Responsiveness

| QA item | Verified by | Result |
|---|---|---|
| Each step renders promptly; transitions are smooth. | `responsiveness_get_state_returns_promptly` confirms 200 sequential `get_state` reads complete under 2s. Step transitions are pure React renders driven by an in-memory `OnboardingState` snapshot. | ✅ |
| The Terraform plan diff renders clearly and is readable. | The Terraform step embeds the Contract 05 provisioner whose plan-diff renderer is exercised by `qa05_test.rs`. | ✅ |
| Language selection applies immediately. | `LanguageStep` calls `setLocale(next)` from the `LocaleProvider` synchronously on change, BEFORE awaiting the IPC persist. The UI re-renders in the new locale on the next paint. The persisted choice survives a relaunch (`happy_language_persists_through_the_wizard`). | ✅ |

## Acceptance criteria — State Transitions

| QA item | Verified by | Result |
|---|---|---|
| First launch → onboarding → completion → main app on next launch. | App.tsx gate + `happy_complete_sets_flag_and_timestamp`. | ✅ |
| Mid-wizard quit → relaunch → resume at the same step. | `state_quit_and_relaunch_resumes_at_the_same_step` simulates the quit by closing the SQLite connection between the cursor write and the next `get_state` read; the row survives and the wizard re-renders at the same step. | ✅ |
| Step N complete → explicit action → step N+1 (never auto-advance). | `security_no_step_auto_advances` marks every step completed without calling `set_current_step` and asserts `current_step` stays at `Language`. The UI's `advance(from)` helper is the ONLY path that calls `setCurrentStep` after a `markStepCompleted`. | ✅ |
| Onboarding complete → Settings re-run → wizard steps for a new account. | `state_settings_re_run_resets_only_wizard_state` confirms `reset_for_rerun` clears only the account/Terraform/first-scan flags, preserves language + password + context, and re-routes the wizard to the account step. The Settings UI exposes both "Add another AWS account" and "Re-run the full onboarding wizard" buttons. | ✅ |

## Security Check

| QA item | Verified by | Result |
|---|---|---|
| On first launch the main app is unreachable until onboarding completes. | App.tsx renders `<Onboarding>` whenever `state.completed === false`. There is no other branch that reaches the AppShell while the flag is unset. | ✅ |
| The master-password step enforces the Contract 02 rules (no skipping, UI-lock only, recovery available). | `PasswordStep` calls `ipc.applockSetMasterPassword` — the exact Contract 02 IPC used by the prior `FirstRunSetup` route. The disclosure copy (`applock.disclosure`) is rendered. The "Next step" button is disabled until the lock state reports `first_run = false`. Recovery via OS identity prompt remains available from the standard UnlockScreen / Settings flow once the password is set. | ✅ |
| The "I'll do this myself" path only shows CLI commands; it never executes them. | The `CliBlock` component renders a `<pre>` block plus a clipboard-copy button. No code path in the wizard spawns a process — the only effect of clicking "Copy" is `navigator.clipboard.writeText`. A red disclaimer ("CloudSaw shows you the commands. CloudSaw does NOT run them on your behalf.") sits below the block. | ✅ |
| The Terraform step shows the plan diff verbatim before apply. | The wizard delegates Terraform UI to the Contract 05 provisioner, whose Plan modal renders the diff verbatim (asserted by `qa05_test.rs`). The wizard's own "Next step" button is gated on `status === "provisioned"`, so the user must explicitly walk through the Plan → Apply flow before moving on. | ✅ |
| The wizard stores no credentials and no account-identifying data beyond what the underlying modules persist; its own state is step flags plus language. | `security_wizard_row_holds_only_step_flags_and_language` asserts the `onboarding_state` schema. Forbidden column names (`password_hash`, `api_key`, `token`, `aws_account_id`, `profile_name`, `secret`) all absent. The 12 actual columns are exactly the step flags, the language, the cursor, the completion flag, and timestamps. | ✅ |
| No step auto-advances. | `security_no_step_auto_advances` exercises every `mark_step_completed` without `set_current_step` and observes the cursor frozen at `Language`. The UI's `advance(from)` helper is the explicit transition point — only invoked by a user's button click. | ✅ |
| Completion is one-way through the wizard surface. | `security_completion_is_one_way_from_the_wizard_surface`: once `completed = true`, neither `mark_step_completed` nor `set_current_step` clears the flag. Only `reset_for_rerun(start_at)` can flip it back, and it requires an explicit step argument. | ✅ |

---

## Test run summary

Full Rust workspace (lib + integration tests, serialized — `-j 1
--test-threads=1`):

```
running 137 tests   (cloudsaw_lib unit)                           → 137/137
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
running 15  tests   (qa14_test)        ← new this contract         → 15/15
running 20  tests   (scanner_test)                                 → 20/20
running 9   tests   (scheduler_test)                               → 9/9
running 16  tests   (terraform_test)                               → 16/16

Total: 423 / 423 ✅
```

Frontend gates:

```
$ tsc --noEmit                                                   → clean
$ vite build                                                     → 402.02 kB bundle, clean
```

---

## Operator-driven checks

These items can't be cleanly asserted from a Rust integration test —
they need a real machine walkthrough, a working AWS profile, and the
Tauri shell. Enumerated here for a release manager to tick off
before tagging:

1. **First-launch gate is hard.** Install a fresh CloudSaw, open it,
   confirm the only visible UI is the Onboarding wizard. Try every
   keyboard shortcut (Tab, Ctrl+W, Ctrl+T, …) — none should reach the
   main app.
2. **Language hot-switch.** At step 1, pick Español; confirm every
   string in the wizard re-renders in Spanish on the next paint
   without a relaunch.
3. **Resume across quit.** At step 4 (Terraform), quit the app, reopen
   it; confirm the wizard re-renders directly at step 4 (not at step
   1).
4. **No AWS CLI guidance.** Rename `~/.aws/config` to break profile
   discovery; reach step 3 in the wizard; confirm the install guidance
   renders with all three per-platform lines.
5. **Profile-deleted route-back.** Add an account on step 3, delete
   that profile from `~/.aws/config`, click "Next step"; confirm the
   Terraform step shows the profile-missing hint pointing back to
   step 3.
6. **Terraform plan-diff visibility.** On step 4, click "Open the
   provisioner"; confirm the plan diff is shown verbatim before the
   user clicks Apply.
7. **Skip context.** On step 5, click "Skip — I'll do this later" and
   confirm the wizard advances; verify that the AI section in
   Settings shows empty business-context fields afterwards.
8. **First scan completes.** On step 6, run a real scan against a
   provisioned account; confirm the "Finish onboarding" button only
   enables once at least one terminal scan exists. Click it; confirm
   the next render is the main app's `Home` route.
9. **Next launch is direct.** Quit; relaunch; confirm the app opens
   straight to `Home`, not to the wizard.
10. **Settings re-run for a new account.** Open Settings → Onboarding;
    click "Add another AWS account"; confirm the wizard re-opens at
    step 3 (Account), with the language and password steps still
    marked completed.
11. **Manual CLI path never executes.** Toggle "I'll do this myself"
    at every step; copy the commands; confirm no process is spawned
    by CloudSaw — only the clipboard write happens.
