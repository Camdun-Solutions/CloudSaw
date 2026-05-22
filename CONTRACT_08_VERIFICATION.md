# Contract 08 — Knowledge Base & Compliance Mapping: Verification Summary

> Branch: `feature/08-knowledge-base`
> QA contract: `C08-knowledge-base-QA.md`
> Tested on: Windows 11, Rust 1.x, `cargo test -j 2`

## What was built

A Rust `knowledgebase` module exposing the contract-mandated public
surface (`get_article`, `list_articles`, `get_control_mappings`,
`list_frameworks`, `check_for_kb_update`, `apply_kb_update`) plus a
companion settings surface (`get_refresh_settings`,
`set_refresh_settings`). The module ships:

- **36 bundled markdown articles** under
  `src-tauri/knowledgebase/articles/` covering the most common and
  highest-severity ScoutSuite finding types across IAM, S3, EC2, VPC,
  RDS, CloudTrail, KMS, CloudFront, ELB, and Lambda. Each article has
  the seven canonical H2 sections (Description, Risk, Detection Logic,
  Remediation, Terraform Fix, AWS CLI Fix, False Positives).
- **A bundled mappings dataset** (`mappings.json`) linking each finding
  to SOC 2, ISO 27001, HIPAA, and NIST controls.
- **An opt-in remote refresh** with the strict default-OFF behaviour
  the contract requires: HTTPS-only fetches of a JSON manifest with
  a stable validated shape, swap-on-success semantics, and full
  bundled fallback on any failure.
- **Eight Tauri commands** registered in `lib.rs`, wired through
  `ipc/mod.rs` with the standard `AppError` mapping; the refresh
  commands run on a tokio blocking worker so they don't block the
  Tauri runtime.

The bundled content is `include_str!`'d so the binary is a fully
self-contained offline source of truth, and a build-time check in
`build.rs` emits `cargo::warning` for any article over 64 KiB.

## QA results — every section of `C08-knowledge-base-QA.md`

### Happy Path
| Item | Result | Backing test |
|------|--------|--------------|
| `get_article` returns populated article for covered finding | PASS | `qa_happy_get_article_populates_known_finding` |
| `list_articles` enumerates all bundled articles | PASS | `qa_happy_list_articles_enumerates_full_bundled_set` |
| `get_control_mappings` returns SOC 2 + ISO 27001 + HIPAA + NIST | PASS | `qa_happy_get_control_mappings_returns_all_four_frameworks` |
| `list_frameworks` returns supported frameworks | PASS | `qa_happy_list_frameworks_returns_bundled_set` |
| Successful remote refresh updates content | PASS | `qa_happy_remote_refresh_replaces_content_when_applied` |

### Error States
| Item | Result | Backing test |
|------|--------|--------------|
| Uncovered finding → default article, `matched=false`, no error | PASS | `qa_error_uncovered_finding_returns_default_with_matched_false` |
| Article missing H2 sections → loads with empty strings, no error | PASS | `qa_error_article_missing_sections_loads_with_empty_strings` |
| Article with unexpected H2 → captured in `unmatched_sections` | PASS | `qa_error_article_with_unexpected_h2_lands_in_unmatched_sections` |
| Remote refresh fails → bundled baseline retained + clear notice | PASS | `qa_error_remote_refresh_failure_retains_bundled_baseline`, `qa_error_remote_refresh_invalid_content_retains_bundled` |
| Duplicate finding-ID markdown → clear startup error | PASS | `registry::tests::bundled_set_loads_without_duplicates` + `qa_error_duplicate_article_id_is_rejected_in_remote_bundle` |

### Responsiveness
| Item | Result | Backing test |
|------|--------|--------------|
| Bundled articles + mappings load at startup quickly | PASS (sub-250ms ceiling) | `qa_responsiveness_bundled_loads_quickly_at_startup` |
| Article + mapping lookups return promptly (in-memory cache, no per-request disk reads) | PASS (400 lookups < 500ms) | `qa_responsiveness_subsequent_lookups_are_fast`, `qa_responsiveness_no_disk_reads_after_cache_warmup` |

### State Transitions
| Item | Result | Backing test |
|------|--------|--------------|
| Bundled → refresh enabled → successful refresh → updated content | PASS | `qa_state_bundled_to_remote_to_failure_to_revert` |
| Bundled → refresh attempted → failure → baseline retained | PASS | same test |
| Framework set → new framework data added → appears with no code change | PASS | `qa_state_new_framework_appears_via_data_only` (the test bundle adds PCI-DSS; it appears in `list_frameworks` + `get_control_mappings` immediately) |

### Security Check
| Item | Result | How verified |
|------|--------|--------------|
| Refresh is opt-in and default OFF | PASS | `qa_security_refresh_defaults_off`; default settings row has `enabled=false`, `remote_active=false` |
| Refresh sends no user/account data — public docs only | PASS | The `Fetcher` trait signature is `fetch_bytes(&str) -> Result<Vec<u8>>` — there is no body or header path. `qa_security_fetcher_only_receives_repo_url_no_account_data` asserts the URL passed to the fetcher is exactly the configured repo URL with no appended query/path |
| Refresh validates received content before replacing bundled | PASS | `qa_security_invalid_content_does_not_replace_bundled` (rejected JSON leaves bundled set intact) |
| Bundled baseline retained as offline fallback | PASS | `qa_error_remote_refresh_failure_retains_bundled_baseline` |
| Markdown returned as raw strings; module does not render HTML | PASS | `qa_security_articles_returned_as_raw_markdown_not_html` (raw `# ` / triple-backtick fences survive; no HTML tags) |
| Bundled content works fully offline | PASS | `qa_security_bundled_works_fully_offline` (lists, mappings, articles all served from in-memory cache without any network) |
| Control-mapping format accepts new frameworks as data (no code change) | PASS | `qa_state_new_framework_appears_via_data_only` |

## Additional security guarantees applied

- **URL allow-listing.** `storage::set_repo_url` rejects anything other
  than `https://` or `file://` (the latter is a power-user test seam),
  blocks URLs over 2 KB, and rejects control characters. Validated by
  `qa_security_set_refresh_settings_rejects_non_https_url` and
  `qa_security_storage_url_validation_rejects_javascript_url`.
- **Finding-id grammar.** `validate_finding_id` (and the registry's
  `is_valid_finding_id`) restrict finding IDs to lowercase ASCII +
  digits + `-` + `_`, blocking path traversal in cache filenames.
  Validated by `registry::tests::remote_content_with_traversal_id_is_rejected`.
- **Response-size cap.** `ReqwestFetcher` rejects payloads over 16 MiB
  so a hostile upstream can't exhaust memory.
- **User-only cache permissions.** The on-disk remote cache
  (`<app_data>/knowledgebase/`) is created via
  `db::paths::ensure_user_only_dir` (mode 0700 on Unix), matching the
  rest of the app's storage discipline (CLAUDE.md §4.5).
- **No new schema.** The module reuses the generic `settings` table
  (migration 0001) with `knowledgebase.*` keys. No new SQL migration
  was added, so no pre-migration backup churn for users on existing
  databases.

## Regression check

`cargo test -j 2` ran the full suite. All targets green:

```
lib unittests                86 passed
accounts_test                24 passed
applock_test                 17 passed
auth_test                    11 passed
findings_test                20 passed
knowledgebase_test           26 passed   (NEW)
migrations_test               5 passed
qa05_test                    19 passed
qa06_test                    23 passed
scanner_test                 20 passed
terraform_test               16 passed
Doc-tests                     0 passed
```

`cargo clippy --lib --tests` produced no new warnings from this
contract's code; the one pre-existing `manual_contains` lint in
`src/accounts/validation.rs` is unrelated to Contract 08.

## Files changed

- `src-tauri/src/knowledgebase/mod.rs` — public surface + bootstrap
- `src-tauri/src/knowledgebase/types.rs` — IPC types
- `src-tauri/src/knowledgebase/error.rs` — typed error enum
- `src-tauri/src/knowledgebase/parser.rs` — markdown section parser
- `src-tauri/src/knowledgebase/bundled.rs` — compile-time content
  manifest
- `src-tauri/src/knowledgebase/registry.rs` — in-memory cache + load
  pipeline
- `src-tauri/src/knowledgebase/storage.rs` — settings + on-disk cache
- `src-tauri/src/knowledgebase/refresh.rs` — opt-in remote refresh
- `src-tauri/knowledgebase/articles/*.md` — 36 bundled articles
- `src-tauri/knowledgebase/mappings.json` — bundled framework dataset
- `src-tauri/src/errors.rs` — added `Kb*` AppError variants
- `src-tauri/src/ipc/mod.rs` — registered 8 new commands
- `src-tauri/src/lib.rs` — registered IPC commands + KB bootstrap
- `src-tauri/Cargo.toml` — added `reqwest` direct dependency (already
  transitive; added with `default-features = false` + minimal feature
  set)
- `src-tauri/build.rs` — build-time oversized-article warning
- `src-tauri/tests/knowledgebase_test.rs` — 26-test QA-aligned
  integration suite
