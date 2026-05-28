# Contract 13 Verification — AI Suggestion Layer

Contract: `cloud-saw-contracts/C13-ai-suggestions.md`
QA contract: `cloud-saw-contracts/C13-ai-suggestions-QA.md`
Branch: `feature/13-ai-suggestions`
Verifier: automated test suite (`src-tauri/tests/qa13_test.rs`) plus the
operator-driven checks called out below.

Contract 13 delivers a fully **opt-in, default-OFF** AI suggestion
layer. With no provider key connected the layer is dormant and no
network call is attempted. When a user does connect their own
Anthropic or OpenAI key, every request is built **by construction**
from category-level data plus structured business context, and every
request is preceded by a mandatory **inline preview panel** that
shows the exact bytes that would be transmitted. (PR #58 flattened
this from a modal hop to an inline disclosure below the finding row;
every byte the modal showed still renders before any send.)

---

## What landed in this contract

**Rust modules**

| Module | Purpose |
|---|---|
| [`ai/`](src-tauri/src/ai/mod.rs) | Public surface: `get_settings`, `set_provider`, `set_provider_key`, `clear_provider_key`, `has_provider_key`, `set_business_context`, `prepare_request`, `send_request`. |
| [`ai/key.rs`](src-tauri/src/ai/key.rs) | Keychain-backed PAT storage at `cloudsaw.llm_api_key` (`anthropic` and `openai` rows). `zeroize::Zeroizing<String>` end-to-end. |
| [`ai/context.rs`](src-tauri/src/ai/context.rs) | Settings-table reads/writes for the structured business-context fields; computes the "looks identifying" flags. |
| [`ai/builder.rs`](src-tauri/src/ai/builder.rs) | Request builder. Produces the EXACT bytes that would be transmitted from finding TYPE (rule key + service + severity + category) + structured context. Never touches `resource_path` or any real identifier. |
| [`ai/client.rs`](src-tauri/src/ai/client.rs) | Anthropic + OpenAI dispatch behind a `Transport` trait. Maps 401/403 → `KeyInvalid`, 429 → `RateLimited`, 5xx → `Server(u16)`. Returns the suggestion verbatim — no swap-back of placeholders. |
| [`ai/types.rs`](src-tauri/src/ai/types.rs) | Public types crossing IPC: `AiSettings`, `BusinessContext`, `ContextFlags`, `FindingDigest`, `AiRequestPreview`, `AiSuggestion`. |

**Migration**

`src-tauri/migrations/0010_ai_context.sql` — six rows in the existing
`settings` table for provider selection + business context fields. All
non-secret. No request/response content is mirrored into SQLite.

**Keychain registry**

`keychain::LLM_KEY_SERVICE = "cloudsaw.llm_api_key"` with two `account`
slots (`anthropic`, `openai`). Both are appended to the panic-wipe
registry so Contract 11's panic removes the AI key alongside every
other CloudSaw secret.

**IPC surface** (registered in `src-tauri/src/lib.rs`):

- `ai_get_settings`, `ai_set_provider`, `ai_set_provider_key`,
  `ai_clear_provider_key`, `ai_has_provider_key`
- `ai_set_business_context`
- `ai_prepare_request` (preview build)
- `ai_send_request` (uses the same preview bytes the UI displayed)

**Frontend additions**

- `src/routes/dashboard/FindingsView.tsx::AiPreviewInline` —
  mandatory inline preview panel rendered below the AI suggestion CTA
  showing provider, model, system prompt, user message, finding
  digest, business context, identifying-field flags, and the
  placeholder list. "Send to provider" / "Cancel — send nothing". PR
  #58 inlined this; until then the same content lived in a
  `AiRequestPreviewModal` component which has been removed.
- `src/routes/Settings.tsx` — new AI section: provider picker, key
  entry (password input), provider-disclosure block, business-context
  fields with per-field "this will be sent" warnings.
- `src/routes/dashboard/FindingsView.tsx` — per-finding "AI suggestion
  (opt-in)" button below the KB remediation, visually distinct (dashed
  border + grey background), with an "AI-generated, unreviewed" badge
  and a placeholder reminder on the rendered suggestion.

---

## Acceptance criteria — Happy Path

| QA item | Verified by | Result |
|---|---|---|
| With a connected provider key, clicking "AI suggestion" shows the request preview, and on Send returns a suggestion. | `happy_with_connected_key_prepare_returns_preview_and_send_returns_suggestion` exercises `prepare_request` → `client::send_with(FakeTransport)` and asserts the captured request body equals the preview body verbatim. The UI splits the same operation across `aiPrepareRequest` → `AiPreviewInline` → `aiSendRequest` (PR #58 inlined the preview). | ✅ |
| The suggestion is clearly labeled AI-generated and unreviewed, distinct from the KB article. | The `AiSuggestionBlock` renders an "AI-generated, unreviewed" pill, a disclaimer line, a placeholder reminder, and lives inside a dashed-border grey panel separate from the KB article. | ✅ |
| Business context set in Settings is reflected in the built request. | `happy_business_context_is_reflected_in_built_request` confirms `industry`, `environment_type`, `compliance`, `risk_tolerance`, `team_size` all appear in the user message. | ✅ |
| The provider-disclosure notice appears on key connection. | `security_disclosure_content_locale_keys_exist` confirms the `ai.disclosure.body` string contains the required phrases ("your chosen provider", "CloudSaw", "cannot control", "AI-generated"). The Settings AI section renders this body in a red-bordered block above the key entry. | ✅ |

## Acceptance criteria — Error States

| QA item | Verified by | Result |
|---|---|---|
| No provider key → "AI suggestion" directs the user to connect a key; no request attempted. | `error_no_provider_no_request_attempted` + `error_no_provider_key_no_request_attempted`. The UI's `startAiSuggestion` short-circuits to `t("ai.error.no_provider_key")` before touching the IPC. | ✅ |
| Invalid/expired key → clear actionable error; no silent resend loop. | `error_invalid_key_yields_actionable_code_no_retry_loop`. The fake transport returns `KeyInvalid`; the suggestion handler surfaces it once and stops. Localized copy points to Settings → AI. | ✅ |
| Provider API error/timeout → clear error; KB article still usable. | `error_rate_limit_and_network_have_distinct_codes` confirms the code mapping. The `AiSuggestionBlock` renders the error in its own subpanel; the KB `<ArticleBody>` above it remains untouched. | ✅ |
| User cancels at the preview → nothing is sent. | `error_cancel_at_preview_modal_sends_nothing` + `state_button_clicked_then_preview_shown_then_send_or_cancel_branch`. The inline preview's "Cancel" button only calls `onCancelPreview` (which clears the preview state); it never invokes `onSend`. `prepare_request` itself is build-only and makes no network call. | ✅ |
| Identifying business-context field → flagged; visible in the preview. | `error_identifying_context_field_is_flagged_and_visible_in_preview` confirms the `flags.industry_identifying` / `flags.compliance_identifying` bits and the verbatim presence of the values in the preview body. The Settings UI shows per-field red warnings; the inline preview shows a top-of-panel flag block. | ✅ |

## Acceptance criteria — Responsiveness

| QA item | Verified by | Result |
|---|---|---|
| The request-preview panel renders the full transmitted text clearly. | The inline preview's `<pre>` blocks for system prompt and user message have `max-h-48` / `max-h-64` + `overflow-auto whitespace-pre-wrap` so the full content is reviewable. The finding digest + business context + flags + placeholders each get their own block. | ✅ |
| A suggestion request reports progress and resolves without an indefinite hang. | `responsiveness_prepare_request_returns_promptly` covers the prep step (< 2s). The send step is gated by `reqwest`'s 60s total timeout (`client.rs`) so an unresponsive provider can't hang the UI. | ✅ |

## Acceptance criteria — State Transitions

| QA item | Verified by | Result |
|---|---|---|
| No key → key connected → AI suggestion becomes available. | `state_no_key_then_key_connected_then_ai_request_is_available`. The Dashboard's `AiSuggestionBlock` enables the button iff `aiSettings.key_connected`. | ✅ |
| Key connected → key cleared → AI suggestion returns to the connect-a-key prompt. | Same test: after `ai::clear_provider_key`, `prepare_request` rejects with `NoProviderKey`; the UI re-disables the button and shows the "connect a key in Settings → AI" hint. | ✅ |
| Button clicked → preview shown → Send → suggestion / Cancel → nothing sent. | `state_button_clicked_then_preview_shown_then_send_or_cancel_branch` covers the full state machine. The fake captures exactly one request after Send and zero after Cancel. | ✅ |

## Security Check

| QA item | Verified by | Result |
|---|---|---|
| With no provider key connected, no AI code path makes any network call. | `security_with_no_key_no_ai_code_path_makes_a_network_call`. Both `prepare_request` and `send_request` short-circuit at the gate before any HTTP client is constructed. `prepare_request` is pure build (reads SQLite + keychain only). | ✅ |
| The provider key is stored only in the OS keychain as `cloudsaw.llm_api_key`; absent from SQLite, config files, logs, and URLs. | `security_key_lives_only_in_keychain_registry_includes_both_providers`. Writes the key, scans every `settings` row, asserts no `sk-ant-…` substring leaks. The registry snapshot includes both provider rows so the panic wipe sweeps them. | ✅ |
| Every AI call is preceded by the request-preview panel; the call proceeds only on explicit user action. | `security_every_ai_call_is_preceded_by_preview_and_uses_the_same_bytes`. The IPC splits `prepare` and `send` into two calls; the UI passes the EXACT preview value to the second call, and the transport captures it byte-for-byte. | ✅ |
| The transmitted request contains no raw ARNs, bucket names, account IDs, or user-chosen identifiers. | `security_transmitted_request_has_no_raw_arn_or_account_id_or_bucket`. Seeds a finding with `arn:aws:s3:::very-secret-bucket-name` and `aws_account_id=111122223333`; the built `user_message` contains neither and uses `[REDACTED-BUCKET-NAME]` instead. The builder reads `rule_key` / `service` / severity / counts — never `resource_path`. | ✅ |
| No real-value↔placeholder substitution map exists; placeholders are not swapped back. | `security_no_real_value_to_placeholder_map_exists_anywhere`. The `placeholders_used` field carries CATEGORY LABELS (`[REDACTED-BUCKET-NAME]`), never real values. The client returns the suggestion markdown unchanged — a response that mentions `[REDACTED-BUCKET-NAME]` stays that way. | ✅ |
| The provider-disclosure notice plainly states the provider relationship and CloudSaw's lack of control over provider data handling. | `security_disclosure_content_locale_keys_exist` asserts the locale body contains the required phrases. The Settings section renders the block in red-bordered prominence above the key entry. | ✅ |
| AI request/response content is not written to logs tied to identifiers. | `security_ai_request_content_is_not_written_to_eventlog`. The public `send_request` records an event-log row with the form `"AI suggestion received from {provider} (model {model})"` — no `user_message` body, no suggestion text. Counter-test asserts that distinctive substrings from both never appear in any event-log row's `summary` or `detail`. | ✅ |

---

## Test run summary

Full Rust workspace (lib + integration tests, serialized — `-j 1
--test-threads=1`):

```
running 134 tests   (cloudsaw_lib unit)                           → 134/134
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
running 18  tests   (qa13_test)        ← new this contract         → 18/18
running 20  tests   (scanner_test)                                 → 20/20
running 9   tests   (scheduler_test)                               → 9/9
running 16  tests   (terraform_test)                               → 16/16

Total: 405 / 405 ✅
```

Frontend gates:

```
$ tsc --noEmit                                                   → clean
$ vite build                                                     → 380.33 kB bundle, clean
```

---

## Operator-driven checks

These items can't be cleanly asserted from a Rust integration test —
they need a real API key, the actual provider endpoint, or a UI
walkthrough. Enumerated here for a release manager to tick off before
tagging:

1. **Default-dormant on a fresh install.** Install CloudSaw, never
   open Settings → AI. Confirm there is no Settings change that would
   make the AI button work without explicit user action. The button
   on a finding shows "AI suggestion (opt-in)" disabled with the
   "Connect a provider key in Settings → AI" hint.
2. **Anthropic round-trip.** Paste a real Anthropic key, set business
   context, click "AI suggestion" on a finding. Confirm the preview
   modal shows the system prompt + user message + digest + context +
   placeholders. Click Send. Confirm a suggestion renders with the
   "AI-generated, unreviewed" label, the disclaimer, the placeholder
   reminder, and the token usage line.
3. **OpenAI round-trip.** Repeat with a real OpenAI key.
4. **Externally-revoked key.** Revoke the key in the provider's UI
   while CloudSaw is open. Trigger an AI suggestion. Confirm the
   error reads "Your provider rejected the API key. Open Settings →
   AI and replace it." and the KB article above remains usable.
5. **Cancel at preview.** Open the inline preview, click "Cancel —
   send nothing". Confirm zero outbound HTTPS to the provider host
   (browser dev tools / proxy log / `netstat`).
6. **Identifying flag visibility.** Set `industry` to "Acme Corp",
   add a numbered tag to `compliance`. Open the inline preview.
   Confirm the red-bordered flags panel appears, both items
   highlighted.
7. **Panic interaction.** Configure an AI key, then run Settings →
   Panic. Confirm the key is removed from the OS keychain (verify via
   Keychain Access / `secret-tool search` / Credential Manager) and
   that the next CloudSaw install starts with "Key not connected".
