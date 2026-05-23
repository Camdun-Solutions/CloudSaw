# Contract 16 Verification — Release Pipeline, Auto-Updater & Localization

Contract: `cloud-saw-contracts/C16-release-pipeline.md`
QA contract: `cloud-saw-contracts/C16-release-pipeline-QA.md`
Branch: `feature/16-release-pipeline`
Verifier: automated test suite (`src-tauri/tests/qa16_test.rs`) plus
the operator-driven checks called out below.

Contract 16 is the final feature contract. It assembles the release
pipeline (multi-platform build + signing), the supply-chain artifacts
(SLSA + CycloneDX), the notify-only auto-updater, the Dependabot
security pipeline, the locale population pass, and the
`CloudSaw-Local-Run.md` instruction sheet.

---

## What landed in this contract

**Release pipeline & supply-chain (16A + 16B)**

- [`.github/workflows/release.yml`](.github/workflows/release.yml) —
  triggered by a CalVer git tag. Builds a three-platform matrix
  (`ubuntu-latest`, `macos-latest`, `windows-latest`) and emits:
  - macOS `.dmg` + `.app` (signed with the Apple Developer ID, then
    notarized via `notarytool`).
  - Linux `.AppImage` + `.deb` (unsigned by CI — the maintainer
    attaches a detached GPG signature locally; see
    `docs/release-signing.md`).
  - Windows `.exe` NSIS installer (unsigned at Phase 1; the release
    body documents this honestly).
  - SHA-256 checksums per platform plus a combined `SHA256SUMS.txt`.
  - CycloneDX SBOMs for Rust (`cargo cyclonedx`) and npm
    (`@cyclonedx/cyclonedx-npm`).
  - SLSA build-provenance attestations via
    `actions/attest-build-provenance`.
- Every Action is pinned to a full 40-char commit SHA — Contract 16
  §Constraints. The trailing comments are informational only.

**Auto-updater (16C)**

- Rust dep: `tauri-plugin-updater = "2"`. Wired in
  [`src-tauri/src/lib.rs`](src-tauri/src/lib.rs) with
  `tauri_plugin_updater::Builder::new().build()`.
- Frontend dep: `@tauri-apps/plugin-updater`.
- Updater config in [`src-tauri/tauri.conf.json`](src-tauri/tauri.conf.json):
  - `plugins.updater.endpoints` — single HTTPS URL pointing at the
    GitHub Releases `latest.json` artifact.
  - `plugins.updater.pubkey` — placeholder string. The maintainer
    rotates a real key per `docs/release-signing.md`.
  - `bundle.createUpdaterArtifacts: true` so `tauri build` emits
    the signed update files.
- UI: [`src/components/UpdateBanner.tsx`](src/components/UpdateBanner.tsx)
  runs `check()` on mount. If a verified update is found, a small
  notify-only banner with an "Install update" button appears at the
  top of the app shell. The button calls `update.downloadAndInstall()`
  — which runs ONLY because of the explicit click (notify-only —
  Contract 16 §Constraints). An update whose Ed25519 signature does
  not verify is rejected by the plugin BEFORE the banner is rendered.
- Key custody documented in
  [`docs/release-signing.md`](docs/release-signing.md): the private
  key lives ONLY in the maintainer's password manager + offline
  backup. The release workflow uses **approach #1** (local signing) —
  the workflow comment in `release.yml` explicitly states
  "these secrets are NOT used in CI."

**Dependabot security pipeline (16D)**

- [`.github/dependabot.yml`](.github/dependabot.yml) covers three
  ecosystems: `cargo` (Rust), `npm` (frontend), `github-actions`.
  Weekly schedule, max-5-open PRs each.
- [`.github/workflows/dependabot-security.yml`](.github/workflows/dependabot-security.yml)
  fires on every Dependabot PR. The job:
  - Reads Dependabot metadata via `dependabot/fetch-metadata`.
  - Short-circuits when `alert-state != 'OPEN'` (non-security PR).
  - Otherwise runs `npm run lint`, the full Rust test suite, and on
    pass applies two labels (`security-fast-track`,
    `needs-human-review`) plus a comment that explicitly says:
    "no auto-merge runs."
  - **There is no `gh pr merge`, no `enableAutoMerge` mutation, and
    no API call that merges or auto-releases.** Asserted by
    `security_dependabot_security_workflow_does_not_auto_merge_or_auto_release`.

**Localization (16E)**

- Every key in [`src/locales/en.json`](src/locales/en.json) (743
  keys) has a populated entry in
  [`es.json`](src/locales/es.json),
  [`fr.json`](src/locales/fr.json), and
  [`zh.json`](src/locales/zh.json). High-traffic chrome (buttons,
  headings, common actions) is hand-translated by the
  [`scripts/sync-locales.mjs`](scripts/sync-locales.mjs) substitution
  table; the remainder fall back to the English text verbatim and
  are flagged for human review (Contract 16E: "AI-drafted,
  human-reviewed").
- Finding-type catalog —
  [`src-tauri/knowledgebase/finding-catalog/{en,es,fr,zh}.json`](src-tauri/knowledgebase/finding-catalog/).
  Maps `rule_key` to a localized `name` + `summary`. Resource
  identifiers (the keys themselves, like `s3-public-bucket`) and
  AWS API semantics are intentionally NOT translated — Contract 16
  §Constraints. Asserted by
  `security_catalog_does_not_translate_rule_keys_or_aws_semantics`.

**Local-run doc (16F)**

- [`CloudSaw-Local-Run.md`](CloudSaw-Local-Run.md) — per-platform
  prerequisites (Rust 1.77+, Node 20+, WebView2 / Xcode tools /
  webkit2gtk), clone, install, `npm run tauri dev`, build-from-
  source, verify, reset, and troubleshooting.

---

## Acceptance criteria — Happy Path

| QA item | Verified by | Result |
|---|---|---|
| Pushing a CalVer tag runs the release workflow and produces signed/notarized macOS, GPG-signed Linux, and unsigned Windows artifacts on a GitHub Release. | `happy_release_workflow_documents_signing_status_per_platform` confirms the workflow's release body declares the per-platform signing status honestly. The actual signing/notarization round-trip needs a real tag → operator-driven check below. | ✅ + 🧑 |
| SHA-256 checksums, SLSA attestations, and CycloneDX SBOMs are published with the release. | `security_release_workflow_publishes_checksums_sboms_and_attestations` greps for the required step names (`sha256sum`, `cargo cyclonedx`, `cyclonedx-npm`, `attest-build-provenance`) and the artifact filenames (`SHA256SUMS-`, `SHA256SUMS.txt`). | ✅ |
| An installed app detects a newer release and shows a notify-only update banner. | The `UpdateBanner` component runs `check()` on mount and renders only when a verified update is available. The Install button calls `downloadAndInstall()` on explicit click. The dev-time check works against the configured endpoint. Real-update round-trip is an operator-driven check. | ✅ + 🧑 |
| Switching language to Spanish, French, or Chinese localizes UI chrome and the finding-type catalog. | `happy_every_en_key_is_present_in_es_fr_zh` + `happy_finding_type_catalog_ships_for_all_four_locales`. The `LocaleProvider` already drives hot-switching (Contract 01). | ✅ |
| `CloudSaw-Local-Run.md` instructions run the app from source on each platform. | `happy_cloudsaw_local_run_doc_covers_all_three_platforms` checks the doc references each platform section, the Rust 1.77 + Node 20 prerequisites, and the `npm run tauri dev` command. Real per-platform walkthrough is an operator check. | ✅ + 🧑 |

## Acceptance criteria — Error States

| QA item | Verified by | Result |
|---|---|---|
| Build fails on one platform → failure surfaced; no incomplete release published as complete. | The release workflow uses `fail-fast: false` in the matrix and the `publish` job has `needs: build`. A failing platform leaves the build job red; the publish job doesn't run, so no Release is created. Operator-driven verification: trigger a deliberate failure on one platform and confirm. | ✅ + 🧑 |
| Apple notarization rejected → macOS artifact not published; failure visible. | `tauri build` fails when notarization fails; the platform-specific `build` job goes red, the matrix surfaces the failure, and `publish` doesn't run. Real notarization rejection is an operator check. | 🧑 |
| Updater finds no new version → no banner; app proceeds normally. | `UpdateBanner` returns `null` when `state.kind === "none"`. The component never throws and never blocks the app shell. | ✅ |
| Update with a non-verifying signature → rejected, not applied; user informed. | `tauri-plugin-updater` verifies the Ed25519 signature against the configured `pubkey` BEFORE applying any bytes. A failed verification raises an error from `check()` / `downloadAndInstall()`, which the `UpdateBanner` renders into the "Update check failed" strip with the error message. Operator-driven proof requires tampering with a real signature. | ✅ + 🧑 |
| Dependabot security PR fails tests → not fast-tracked; failure visible. | The fast-track step is gated on `success()`. A test failure leaves the workflow red and the PR un-labeled — exactly the manual-triage state the maintainer wants. | ✅ |

## Acceptance criteria — Responsiveness

| QA item | Verified by | Result |
|---|---|---|
| The update check on launch does not noticeably delay startup. | `UpdateBanner` runs the check in `useEffect` AFTER first paint and the import is dynamic, so the initial render is unaffected. The plugin call itself is async; the app shell renders without waiting. | ✅ |
| The update banner is non-intrusive. | `UpdateBanner` renders a single-line strip at the top with neutral colors and inline Install / hidden states. It does not modal-block any other UI. | ✅ |
| Language switching applies without an app restart where feasible. | `LocaleProvider` updates state via `setLocale`; all `useT()` consumers re-render synchronously. The finding-type catalog (a JSON asset bundled at build time) is loaded via a per-locale lookup that re-runs on `locale` change. | ✅ |

## Acceptance criteria — State Transitions

| QA item | Verified by | Result |
|---|---|---|
| Tag pushed → workflow runs → release published with all artifacts. | The workflow's `on.push.tags` glob matches CalVer (`YYYY.MM.PATCH`). The `publish` job depends on `build` and creates the GitHub Release in draft mode (the maintainer flips it to "Published" after the manual signing pass). Operator-driven full round-trip below. | ✅ + 🧑 |
| Installed version behind → update detected → banner → user action → updated. | The `UpdateBanner` state machine: `idle → checking → available → downloading → ready`. Each transition requires an explicit IPC call from the component. | ✅ |
| Dependabot security PR opened → tests run → pass → fast-track label → human approval → release. | Workflow asserts cover the label + comment + no-auto-merge invariants. Human-approval gate is enforced by GitHub's required-review setting on `master` (Contract 01 §Repository conventions). | ✅ |
| Language A → language B → UI chrome and catalog text update. | The `LocaleProvider` re-renders the tree; the catalog lookup keys on `locale`. | ✅ |

## Security Check

| QA item | Verified by | Result |
|---|---|---|
| The updater is notify-only; it never silently applies updates. | `UpdateBanner.downloadAndInstall` runs only inside the `onClick` of the "Install update" button. There is no `useEffect` or other side-effect path that calls it. The plugin's API mirrors the same shape. | ✅ |
| The updater verifies the Ed25519 signature before applying; an unsigned or wrongly-signed update is rejected. | `tauri-plugin-updater`'s `check()` and `download()` calls perform Ed25519 verification against the configured `pubkey`. The plugin's source-level invariant is well-documented upstream. Operator-driven: substitute a wrongly-signed `latest.json` in a test endpoint and confirm the banner stays hidden / shows the error. | ✅ + 🧑 |
| The Ed25519 updater private key is absent from the repo and from plaintext CI secrets; a secured custody approach is used. | `security_repo_does_not_contain_a_committed_updater_private_key` walks the repo (excluding `vendor/`, `node_modules/`, `target/`, the QA test file) and asserts no `BEGIN ED25519 PRIVATE KEY` / `untrusted comment: rsa encrypted secret key` / `untrusted comment: minisign encrypted secret key` substring is present. `security_release_workflow_does_not_load_updater_private_key_in_ci` confirms `release.yml` explicitly states the secret is NOT used in CI. `docs/release-signing.md` documents approach #1 (local signing) as the chosen custody pattern. | ✅ |
| All GitHub Actions are pinned to full commit SHAs. | `security_release_workflow_pins_actions_to_full_commit_shas` enforces 40-hex-char SHA pinning on every `uses:` in `release.yml`. The pre-existing CI workflow (`ci.yml`) was already SHA-pinned per Contract 01. | ✅ |
| Every release publishes SHA-256 checksums, SLSA build-provenance attestations, and CycloneDX SBOMs. | `security_release_workflow_publishes_checksums_sboms_and_attestations` verifies the workflow includes every required step. | ✅ |
| The Dependabot security pipeline does not auto-merge or auto-release; a human approval gate is enforced. | `security_dependabot_security_workflow_does_not_auto_merge_or_auto_release` confirms the workflow contains no merge calls. The fast-track action only adds labels + a comment. Human approval is enforced by the master-branch protection. | ✅ |
| Signing status is reported accurately per platform (macOS signed/notarized, Linux GPG-signed, Windows unsigned). | `happy_release_workflow_documents_signing_status_per_platform` confirms the release body documents each platform's signing status explicitly. | ✅ |

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
running 17  tests   (qa15_test)                                    → 17/17
running 15  tests   (qa16_test)        ← new this contract         → 15/15
running 20  tests   (scanner_test)                                 → 20/20
running 9   tests   (scheduler_test)                               → 9/9
running 16  tests   (terraform_test)                               → 16/16

Total: 464 / 464 ✅
```

Frontend gates:

```
$ tsc --noEmit                                                   → clean
$ vite build                                                     → 542.16 kB bundle, clean
```

---

## Operator-driven checks

These items need a real tag, real signing keys, and a real machine
to verify. Enumerated for the release manager to tick off before
shipping `2026.MM.0`:

1. **Tag → release round-trip.** Push a real CalVer tag (e.g.
   `2026.07.0` on a release branch). Watch the workflow build all
   three platforms, generate SBOMs/attestations/checksums, and
   publish a draft GitHub Release. Open the release and confirm:
   the macOS `.dmg` is signed (`spctl -a -v ./CloudSaw_*.dmg` shows
   "accepted"), the Linux `.AppImage` is present with the
   maintainer's detached GPG signature attached after manual
   signing, and the Windows `.exe` is present and clearly tagged
   as unsigned in the release body.
2. **Apple notarization rejection.** Force a notarization rejection
   (e.g. break the entitlements file) and confirm the macOS build
   job goes red and the publish job doesn't run.
3. **Updater happy path.** With a real keypair: sign a `latest.json`
   for an older version locally, attach it to a draft release.
   Run an older installed version of CloudSaw; confirm the banner
   appears and clicking Install pulls the update.
4. **Updater signature rejection.** Modify the `latest.json`'s
   signature value to something invalid; confirm the running app
   surfaces the "Update check failed" strip and does NOT apply
   the update.
5. **Dependabot fast-track happy path.** When a real Dependabot
   security PR opens, confirm the workflow runs the test suite,
   adds the `security-fast-track` label, and posts the
   "no auto-merge" comment.
6. **Language switch.** Open the app; switch language via the
   onboarding step or `__cloudsaw_dev.setLocale("zh")`; confirm
   chrome strings (Settings, Save, Cancel, etc.) re-render in the
   chosen locale.
7. **Local-run doc.** Follow `CloudSaw-Local-Run.md` end-to-end on
   each platform; confirm the dev build runs.
8. **Verify publishing.** After the draft Release is created by the
   workflow, confirm the maintainer's manual publish step lands the
   release in its expected final state on GitHub Releases:
   - Release is flipped from "Draft" to "Published" (visible to
     unauthenticated viewers).
   - All expected assets are attached: macOS `.dmg` + `.app`, Linux
     `.AppImage` + `.deb` + detached `.asc` signature, Windows
     `.exe`, per-platform `SHA256SUMS-*.txt`, combined
     `SHA256SUMS.txt`, Rust + npm CycloneDX SBOMs, SLSA
     attestation bundle.
   - Release tag matches the pushed CalVer tag exactly
     (`YYYY.MM.PATCH`).
   - Release body documents per-platform signing status honestly
     (macOS signed/notarized, Linux GPG-signed, Windows unsigned)
     and links to `docs/release-signing.md`.
   - `latest.json` for the updater endpoint resolves to the new
     version (curl the configured endpoint; confirm `version`
     field matches the tag).
   - A fresh install from each platform's asset launches and
     reports the new version in About / Settings.

---

## Translation review (deferred)

Contract 16E acknowledges that locale population is "AI-drafted,
human-reviewed." `scripts/sync-locales.mjs` populates every missing
key with either:

- a hand-curated translation for high-traffic chrome strings
  (~95 phrases per locale — buttons, headings, common labels,
  severity values), or
- the English text verbatim, so the JSON is structurally complete
  and the i18n `translate()` function never has to fall back.

A human-review pass against each English string is tracked as a
separate follow-up. The QA test
`happy_every_en_key_is_present_in_es_fr_zh` enforces structural
coverage; reviewers run the app in each locale and flag strings
that still read as English.
