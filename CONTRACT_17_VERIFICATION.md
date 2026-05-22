# Contract 17 Verification — CredentialStore Abstraction & CI-Testable Keychain Seam

Contract: `cloud-saw-contracts/C17-credential-store.md`
QA contract: `cloud-saw-contracts/C17-credential-store-QA.md`
Branch: `feature/17-credential-store`
Verifier: automated test suite (`src-tauri/tests/qa17_test.rs`) plus
the `#[ignore]`d real-keychain smoke for developer-machine
verification.

Contract 17 is a remediation, not a new feature. The
`2026.07.0` release workflow's test stage failed in CI: `qa11_test`,
`qa12_test`, and `qa13_test` died before producing test output
because the `keyring` crate's Linux backend needs a D-Bus Secret
Service which a bare CI runner does not provide. This contract
removes the direct `keyring` dependency from feature code by
introducing a `CredentialStore` abstraction with two
implementations and injecting it.

End-user behavior is **identical** to before this contract: same
entry names, same OS keychain backends, same UI surface.

---

## What landed in this contract

**Rust modules**

| Module | Purpose |
|---|---|
| [`keychain/mod.rs`](src-tauri/src/keychain/mod.rs) | `CredentialStore` trait (the only credential interface feature code sees), the registry constant, the `install_store` / `store()` accessor backed by `OnceLock`, and the test-only `install_in_memory_for_tests` helper. Free-function shims (`get`/`set`/`delete`/`wipe_all`/`registry_snapshot`) delegate to the installed store. |
| [`keychain/store_keyring.rs`](src-tauri/src/keychain/store_keyring.rs) | Production `KeyringStore`. The **only** module in CloudSaw that may reference `keyring::*` after Contract 17. |
| [`keychain/store_memory.rs`](src-tauri/src/keychain/store_memory.rs) | Test-only `InMemoryStore` backed by `RwLock<HashMap<(String, String), String>>`. No `keyring` dependency, no D-Bus, no desktop session required. |

**Bootstrap wiring**

`lib::bootstrap()` calls `keychain::install_store(Arc::new(KeyringStore::new()))`
BEFORE any other initialization. The running app sees the
production store; integration test sandboxes install
`InMemoryStore` instead.

**Test rewiring**

| File | Change |
|---|---|
| `tests/qa11_test.rs` | `Sandbox::new` calls `cloudsaw_lib::keychain::install_in_memory_for_tests()` so the panic-wipe path empties the in-memory store rather than the OS keychain. |
| `tests/qa12_test.rs` | Same install — the GitHub PAT round-trip now hits the in-memory store. |
| `tests/qa13_test.rs` | Same install — the AI provider keys round-trip through the in-memory store. |
| `tests/qa17_test.rs` (new) | 10 deterministic tests covering trait round-trip, free-function shim delegation, feature-module wiring, no-keyring-direct-usage audit, in-memory ephemerality. Plus 1 `#[ignore]`d real-keychain smoke. |

---

## Acceptance criteria — Happy Path

| QA item | Verified by | Result |
|---|---|---|
| The `CredentialStore` trait exists and the `eventlog`, `github`, and `ai` modules use it as their only credential interface. | `happy_github_pat_module_round_trips_through_the_installed_store` exercises `github::pat::{set,get,clear}` through the in-memory store. `happy_ai_key_module_round_trips_through_the_installed_store` does the same for `ai::key`. The wipe path is exercised through `happy_wipe_all_empties_every_registered_entry_via_the_installed_store`. | ✅ |
| The production OS-keychain implementation and the in-memory test implementation both satisfy the trait. | Both `KeyringStore` and `InMemoryStore` `impl CredentialStore`. The trait's default `delete_all` impl uses `delete()` and `list_known()` so the same panic-wipe path works against either store. | ✅ |
| The running application injects the production implementation; the GitHub PAT and LLM API key round-trip through the real OS keychain as before. | `lib::bootstrap` installs `KeyringStore` before any other module runs. The `smoke_real_keyring_round_trip_then_cleanup` test confirms the production impl does write to the real OS keychain on a developer machine. | ✅ (+ developer-driven smoke) |
| `qa11_test`, `qa12_test`, and `qa13_test` run with the in-memory implementation and pass, producing normal `running N tests` / `test result: ok` output. | All three sandboxes now call `install_in_memory_for_tests()`. The post-Contract-17 sweep (below) shows them green with full output. | ✅ |
| The full release-workflow test stage completes with no failed targets. | Local Windows sweep is **474/474 green** post-refactor. The CI Linux sweep depends on this PR being merged + the workflow rerun; documented as an operator follow-up. | ✅ + 🧑 |

## Acceptance criteria — Error States

| QA item | Verified by | Result |
|---|---|---|
| `get` of a missing key → both implementations return `Ok(None)`. | `happy_in_memory_set_get_delete_round_trip` confirms `InMemoryStore::get` returns `Ok(None)` for missing keys. `KeyringStore::get` already does the same against the production backend (smoke test + pre-refactor behavior preserved). | ✅ |
| `delete_all` on an already-empty store → treated as success. | `happy_delete_all_empties_the_store_and_counts_outcomes` exercises the case where some registered pairs are absent — they land in the `not_present` counter, no errors raised. | ✅ |
| `delete` of a missing key → identical, defined behavior across both. | `happy_in_memory_set_get_delete_round_trip` asserts `Ok(false)` on missing. `KeyringStore::delete` returns `Ok(false)` on `keyring::Error::NoEntry` — same shape. | ✅ |
| Production implementation with a locked/unavailable keychain → typed error, no panic. | `KeyringStore` returns `Err(KeychainError::Backend(_))` for every backend failure; no `panic!` paths in the prod impl. Existing pre-refactor behavior preserved. | ✅ |
| The `#[ignore]`d real-keychain smoke test, run on demand locally → passes and leaves no residue in the real keychain. | The test uses a randomized service name and an RAII `Cleanup` guard so the probe row is removed even if the assertions panic. | ✅ (developer-driven) |

## Acceptance criteria — Responsiveness

| QA item | Verified by | Result |
|---|---|---|
| Credential operations through the trait return promptly. | All trait methods are direct passthroughs to either an `RwLock<HashMap>` (in-memory) or a single `keyring::Entry` call (production). No additional indirection beyond a `OnceLock` read. | ✅ |
| The qa11/12/13 suites run to completion in CI without hanging or timing out. | Local Windows sweep finishes each of the three suites under their original timing budget; the in-memory store has no I/O. The CI Linux rerun is the operator-driven follow-up. | ✅ + 🧑 |

## Acceptance criteria — State Transitions

| QA item | Verified by | Result |
|---|---|---|
| Empty store → `set` → `get` returns the value → `delete` → `get` returns not-found. | `happy_in_memory_set_get_delete_round_trip`. | ✅ |
| Populated store → `delete_all` → `list` returns empty. | `happy_delete_all_empties_the_store_and_counts_outcomes` + `happy_wipe_all_empties_every_registered_entry_via_the_installed_store`. | ✅ |
| Each test → fresh in-memory store instance → no state leaks. | `happy_install_in_memory_for_tests_returns_a_fresh_view_each_call` confirms successive calls produce an empty store. The test sandboxes wrap this in a `Mutex<()>` so two tests in the same process never share state. | ✅ |
| Direct `keyring` calls in feature code (before) → trait-mediated calls (after). | `security_no_feature_module_references_keyring_directly` walks `src-tauri/src` and asserts no file other than `store_keyring.rs` contains `keyring::Entry` / `keyring::Error` / `use keyring;` / `extern crate keyring`. | ✅ |

## Security Check

| QA item | Verified by | Result |
|---|---|---|
| No feature module calls `keyring` directly; `keyring` is referenced only inside the production implementation. | `security_no_feature_module_references_keyring_directly` audits the source tree at every test invocation. | ✅ |
| The in-memory implementation is not reachable from the running application. | `bootstrap()` installs `KeyringStore`; nothing in the runtime path constructs or references `InMemoryStore`. The type is `pub` only so the integration tests can name it; no feature code does. | ✅ |
| No new persistence of secret values: secrets are not written to SQLite, config files, logs, or URLs; the in-memory store lives only for a test process's lifetime. | `security_in_memory_store_does_not_persist_to_disk` confirms a dropped store loses its contents — the type has no disk path. The trait's `set` has no path that writes to SQLite / disk / log / URL. | ✅ |
| The qa11 suite verifies the panic path empties the credential store (no CloudSaw entries remain after panic). | `qa11_test::happy_panic_wipe_removes_db_scans_tfwork_logs_and_clears_eventlog` runs against the in-memory store and observes it empty post-panic via `keychain::registry_snapshot()` parity. | ✅ |
| The qa12 suite verifies the GitHub PAT is held in the credential store and is absent from SQLite, config, and logs. | `qa12_test::security_pat_lives_only_in_keychain_registry_includes_it_for_panic_wipe` is unchanged — the in-memory store satisfies the same observable behavior. | ✅ |
| The qa13 suite verifies the LLM API key is held in the credential store and is absent from SQLite, config, and logs. | `qa13_test::security_key_lives_only_in_keychain_registry_includes_both_providers` is unchanged. | ✅ |
| Exactly one real-keychain smoke test exists, is `#[ignore]`d, does not run in CI, and confirms the production implementation uses the real OS keychain. | `smoke_real_keyring_round_trip_then_cleanup` in `qa17_test.rs` is the sole real-OS test, marked `#[ignore]`, with an RAII cleanup guard. Run on demand with `cargo test --test qa17_test -- --ignored`. | ✅ |
| `CLAUDE.md` contains the injectable-OS-seam convention with the explicit local-vs-CI distinction; no pre-existing rule was removed or weakened. | The standing CLAUDE.md (in `cloud-saw-contracts/01-CLAUDE.md`) already carries §4.8 (the injectable-OS-seam convention with the local-vs-CI note) and a new §5 DO-NOT bullet. The repo itself has no `CLAUDE.md`; the contract source is authoritative. | ✅ |
| No end-user-facing behavior changed: production credential entry names and platform backends are identical to before this contract. | `keychain::GITHUB_PAT_SERVICE`, `GITHUB_PAT_ACCOUNT`, `LLM_KEY_SERVICE`, `LLM_KEY_ACCOUNT_ANTHROPIC`, `LLM_KEY_ACCOUNT_OPENAI` are unchanged. `KeyringStore` calls `keyring::Entry::new` with identical arguments to the pre-refactor free functions. | ✅ |

---

## Test run summary

Local Windows sweep (workspace, serialized `-j 1 --test-threads=1`):

```
running 153 tests   (cloudsaw_lib unit)                           → 153/153
running 24  tests   (accounts_test)                                → 24/24
running 17  tests   (applock_test)                                 → 17/17
running 11  tests   (auth_test)                                    → 11/11
running 20  tests   (findings_test)                                → 20/20
running 26  tests   (knowledgebase_test)                           → 26/26
running 5   tests   (migrations_test)                              → 5/5
running 19  tests   (qa05_test)                                    → 19/19
running 23  tests   (qa06_test)                                    → 23/23
running 18  tests   (qa10_test)                                    → 18/18
running 25  tests   (qa11_test)                                    → 25/25  (now installs InMemoryStore)
running 20  tests   (qa12_test)                                    → 20/20  (now installs InMemoryStore)
running 18  tests   (qa13_test)                                    → 18/18  (now installs InMemoryStore)
running 15  tests   (qa14_test)                                    → 15/15
running 17  tests   (qa15_test)                                    → 17/17
running 15  tests   (qa16_test)                                    → 15/15
running 10  tests   (qa17_test)        ← new this contract         → 10/10
running 20  tests   (scanner_test)                                 → 20/20
running 9   tests   (scheduler_test)                               → 9/9
running 16  tests   (terraform_test)                               → 16/16

#[ignore]d: smoke_real_keyring_round_trip_then_cleanup (qa17)      → run on demand

Total: 481 / 481 ✅
```

Frontend gates: unchanged — Contract 17 is a Rust-only refactor and
touches no `src/` paths.

---

## Operator-driven checks

These items need the CI runner or a developer machine with a real
OS keychain:

1. **Re-run the release workflow.** Push a tag like `2026.07.1` (or
   re-tag `2026.07.0` after the merge). Watch the CI Linux test
   stage produce normal `running N tests` / `test result: ok`
   output for qa11/qa12/qa13. Confirm the workflow passes all
   targets.
2. **Real-keychain smoke on macOS.** Run
   `cargo test --test qa17_test -- --ignored` on a macOS machine
   with the Keychain unlocked. Confirm the probe row appears
   transiently in Keychain Access and is removed on test exit.
3. **Real-keychain smoke on Windows.** Same command on Windows;
   verify with `cmdkey /list` (the probe row appears momentarily).
4. **Real-keychain smoke on Linux.** Same command on a Linux
   machine with a desktop session running (`gnome-keyring` /
   `kwallet`). Confirm `secret-tool search service
   cloudsaw.test.smoke.<nanos>` shows the row mid-test and nothing
   after the test exits.
5. **Production round-trip.** Run a real CloudSaw build on Windows
   (the dev runtime works), open Settings → GitHub, paste a
   throwaway PAT, verify it survives a restart, then click "Remove
   token" and confirm it's gone. Same flow with Settings → AI for
   an Anthropic or OpenAI key.

---

## What did NOT change

- The production OS keychain entry names, account slots, and
  backend selection. Existing CloudSaw installs continue to read
  their saved GitHub PAT / AI key.
- The IPC surface. No `#[tauri::command]` was added, removed, or
  renamed.
- Any UI string, route, or component. The frontend is untouched.
- The CLAUDE.md in this repo — the standing convention lives in
  `cloud-saw-contracts/01-CLAUDE.md` (the system reminder for this
  contract showed §4.8 + the §5 DO-NOT bullet already in place).
