# cloudsaw.spec — PyInstaller iteration log

A running log of every `hiddenimports` and `datas` addition (and any other
spec adjustment), with the error message that motivated it. Saves
re-debugging when the spec needs touching again — and serves as a paper
trail for reviewers of the Phase 1 PR.

Format:

```
## YYYY-MM-DD — <one-line summary>

**Symptom**: <error message from the iterate.sh run, copy-pasted>
**Change**: <what we added to the spec>
**Why**: <root cause — usually "X module imported via importlib() / __import__() / setuptools entry point, PyInstaller's static analysis missed it" or "data file walked via `__file__`-relative path, needed in `datas`">
```

---

## Initial spec (best-effort starting guess)

The `cloudsaw.spec` shipped in this commit is a first pass. ScoutSuite is
known to have these freezing hazards:

1. **Cloud-provider packages are dispatched dynamically.** `scout.py
   aws ...` reaches `ScoutSuite/__main__.py` which imports the AWS
   provider via a string-driven dispatch. PyInstaller's static analyzer
   sometimes misses these — add to `hiddenimports` as errors appear.

2. **Rule definitions are JSON files walked from `__file__`.** Layer A
   patched the walks to use `ScoutSuite._cloudsaw_paths` helpers, but the
   files themselves must be declared in `datas` so PyInstaller actually
   ships them.

3. **boto3 ships its own data tree.** Every AWS API call loads model
   JSON from `botocore/data/`. Without `--collect-data botocore` (or the
   equivalent in the spec's `datas`), boto3 raises `DataNotFoundError`
   at runtime when the first scan tries to list resources.

4. **HTML report templates** live under `ScoutSuite/output/data/`.
   Captured by `datas` already in the initial spec.

5. **google-cloud-* SDKs** have grpcio shared libraries that PyInstaller
   sometimes mis-handles. The GCP/Azure provider paths are deferred — if
   freezing those causes too much friction we may exclude them from the
   initial bundle and ship AWS-only first. For CloudSaw 2026.5.9 only
   AWS scans matter (Contract 06 is AWS-only); a follow-up can expand
   provider coverage if/when CloudSaw adds GCP/Azure scanning.

---

## Iteration entries

## 2026-05-26 — drop nonexistent `output/template` from datas

**Symptom**: `Unable to find '/scoutsuite/ScoutSuite/output/template' when adding binary and data files.`
**Change**: Removed the `('ScoutSuite/output/template', 'ScoutSuite/output/template')` line from `datas`.
**Why**: That directory doesn't exist upstream — HTML templates live inside `ScoutSuite/output/data/html/` which is already covered by the `output/data` glob. The initial-spec guess was wrong about the upstream layout.

## 2026-05-26 — collect_data_files('policyuniverse')

**Symptom**: `FileNotFoundError: [Errno 2] No such file or directory: '/tmp/_MEIRTHvOy/policyuniverse/data.json'` at first import of the frozen binary (before `--help` runs).
**Change**: Added `datas += collect_data_files('policyuniverse')` after the existing boto3/botocore/cherrypy entries.
**Why**: `policyuniverse` is Netflix's AWS IAM policy parser, used by `ScoutSuite/core/conditions.py`. It loads its own `data.json` sibling on import; PyInstaller's static analysis doesn't auto-bundle data files unless told.

## 2026-05-26 — explicit hiddenimports for per-provider authentication_strategy

**Symptom**: `ModuleNotFoundError: No module named 'ScoutSuite.providers.aws.authentication_strategy'` when invoking the frozen binary with `aws --access-keys ...` (i.e. starting a scan, not just `--help`).
**Change**: Added `ScoutSuite.providers.<cloud>.authentication_strategy` for all seven providers to `hiddenimports`.
**Why**: `ScoutSuite/providers/base/authentication_strategy_factory.py:14` does `__import__(f'ScoutSuite.providers.{provider}.authentication_strategy', fromlist=[strategy_class])`. The `__import__` is a runtime string construct that PyInstaller's static analyzer can't follow, and `collect_submodules('ScoutSuite.providers.aws')` didn't pick this submodule up (unclear why — file exists at the expected path). Explicit listing is the safe fix.

## 2026-05-26 — Linux validation passed (fake-creds scan)

**Test**: Invoked frozen binary with `aws --access-keys --access-key-id AKIA...EXAMPLE --secret-access-key wJalrXUt...EXAMPLEKEY --no-browser --report-dir /tmp/r --regions us-east-1 --services iam`.
**Outcome**: Binary started ScoutSuite ("Launching Scout"), resolved AWS auth strategy ("Authenticating to cloud provider"), constructed a boto3 client, made an HTTPS call to `sts:GetCallerIdentity`, received `InvalidClientTokenId` from AWS, surfaced the error gracefully, and exited cleanly. Validates: PyInstaller extraction, Layer A path patches, botocore service models, policyuniverse data files, AWS provider module tree, network/TLS stack inside the bundle.
**What's still untested**: Behavior with valid credentials against real AWS resources. CloudSaw uses STS-assumed credentials at scan time so this is exercised end-to-end on first install + first scan; if a real-scan-specific issue surfaces, it'll be a missing data file or hidden import for a specific boto3 service module — log here when fixed.

## 2026-05-26 — Windows validation passed (native PyInstaller + fake-creds scan)

**Setup**: `python -m venv .pyi-venv-windows`, `pip install -r pyinstaller-requirements.txt -r requirements.txt`, then ran `pyinstaller cloudsaw.spec` directly on Windows 11 with Python 3.11.9 (64-bit).
**Build**: Succeeded with zero spec changes. PyInstaller produced `dist/scoutsuite.exe` (~96 MB). No additional hidden imports or datas needed for Windows.
**Smoke**: `scoutsuite.exe --help` and `scoutsuite.exe aws --help` both rendered cleanly.
**Fake-creds scan**: Same `aws --access-keys ... AKIA...EXAMPLE ...` invocation → identical outcome to Linux (started ScoutSuite, resolved auth strategy, AWS rejected the token, graceful exit). The spec is genuinely cross-platform; no per-OS branching needed.
**What's still untested**: macOS native build. Same approach (Python 3.11 + venv + native `pyinstaller cloudsaw.spec`) should produce a working binary, but Mach-O signing and the codesign-before-bundle pipeline are macOS-only concerns that will be verified by the release.yml step on the macOS runner.

## 2026-05-29 — Windows Defender false-positive on `ScoutSuite/core/ruleset.py` + spec defenses

**Symptom**: Scans on a Windows dev box failed at startup with
`ModuleNotFoundError: No module named 'ScoutSuite.core.ruleset'`
captured in `<scan-dir>/scoutsuite-stderr.log`. The bundled
`scoutsuite.exe` was missing `ScoutSuite/core/ruleset.pyc` from
its PYZ archive even though the source file was checked into the
repo.

**Root cause**: Windows Defender's ML-based real-time scanner
(ThreatID 2147963851, generic `Trojan:Win32` ML detection)
flagged `vendor/scoutsuite/ScoutSuite/core/ruleset.py` as
suspicious. The triggering shape is the `TmpRuleset.__init__`
pattern that:

  1. Builds a Python dict (`tmp_ruleset = {...}`)
  2. Serializes it via `json.dumps`
  3. Writes the JSON to a `tempfile.TemporaryFile('w+t')`
  4. Reads it back and `exec`-shaped JSON parse

That sequence has a real-malware analogue (drop-payload-to-temp-
then-execute), and Defender's ML model misfires on it. The file
content is unambiguously benign — `git blame` shows it traces
straight to upstream ScoutSuite without CloudSaw-side
modifications other than the import line for the
`_cloudsaw_paths` helper.

Defender quarantines the file the moment PyInstaller's analyzer
tries to read it during build, so PyInstaller silently skips it
and emits a `missing module named ScoutSuite.core.ruleset`
warning in `build/cloudsaw/warn-cloudsaw.txt`. The resulting
binary boots into ScoutSuite, prints "Authenticating to cloud
provider," then dies the moment `from ScoutSuite.core.ruleset
import Ruleset` runs at `ScoutSuite/__main__.py:14`.

**Change**: Three-part defense, all in this PR:

  1. Spec: pre-declared every `ScoutSuite.core`,
     `ScoutSuite.output`, and `ScoutSuite.providers` submodule via
     `collect_submodules` calls. A fail-fast assertion below the
     `collect_submodules` block asserts each module
     `ScoutSuite/__main__.py` statically imports is in the
     resolved hiddenimports list. A missing source file now
     surfaces as a build-time error, not a runtime
     ModuleNotFoundError.

  2. CI verification (release.yml): split the existing
     `verify bundled scoutsuite data files` step into (a) the
     same TOC grep for data files in the outer PKG archive +
     (b) a new `smoke-test bundled scoutsuite (--help)` step
     that actually runs the bundled binary and fails the build
     if it raises `ModuleNotFoundError` / `ImportError`. The
     archive-viewer view ONLY exposes the outer PKG; Python
     modules live in the inner PYZ archive and the old grep
     for `.pyc` paths would have always misreported. The
     execute-then-grep approach catches any silent module drop,
     not just the ruleset one.

  3. Documentation: added `WINDOWS_DEFENDER.md` in this
     directory with the exact PowerShell incantation
     developers can run as Administrator to add a per-path
     exclusion for the repo. The exclusion is the canonical
     Windows workaround — once the project dir is excluded,
     Defender stops quarantining the file during builds.
     End users are NOT affected: the signed release binary
     bundles the module *inside* `scoutsuite.exe`, where
     Defender can't reach it without unpacking the executable.

**Real-creds verification**: pending — the developer who hit the
original failure runs the spec + a CI artifact-built binary
against a real AWS profile post-merge.

## 2026-05-27 — explicit-glob datas for ScoutSuite/data tree (macOS bundling fix)

**Symptom**: `[Errno 2] No such file or directory: '/var/folders/.../T/_MEIEv49Fb/ScoutSuite/core/../data/icmp_message_types.json'` at scan-time on 2026.5.14 on macOS. Once the AWS provider's `SecurityGroups` class definition runs (`vendor/scoutsuite/ScoutSuite/providers/aws/resources/ec2/securitygroups.py:9`), it calls `load_data('icmp_message_types.json', ...)` which walks `ScoutSuite/core/../data/`. Windows bundles produced the same day worked fine.
**Change**: Replaced the `('ScoutSuite/data', 'ScoutSuite/data')` directory tuple with an explicit `os.walk('ScoutSuite/data')` loop that emits one `datas` entry per file. Added fail-fast assertions for `icmp_message_types.json`, `protocols.json`, and `ip-ranges/aws.json` so the build dies at spec-eval if the source tree changes shape.
**Why**: PyInstaller's directory-tuple semantics in `datas` are platform-inconsistent — the Windows builder for 2026.5.9 through 2026.5.14 picked up the entire `ScoutSuite/data/` subtree, but the macOS CI builder silently dropped the non-`__init__.py` files. `pyi-archive_viewer --brief` on the Windows EXE confirmed all 5 data files were bundled there. Explicit per-file globs remove the ambiguity. Bumps coverage from 1 line to 5 (icmp_message_types, protocols, 3× ip-ranges/*.json).
**Real-creds verification**: Rebuilt `dist/scoutsuite.exe` locally on Windows, exit 0 against the `cloudsaw` AWS profile across all 27 services / 157 findings. The same spec, when built by CI on the macOS runner, will now bundle the data tree explicitly.

## 2026-05-26 — explicit hiddenimports for per-provider `.provider` modules

**Symptom**: `Initialization failure: No module named 'ScoutSuite.providers.aws.provider'` at scan-time on a real-credentials macOS scan (2026.5.13). The fake-credentials Phase 1 validation against AKIA…EXAMPLE never hit this code path because AWS rejected the STS token at the auth step, before `__main__.py:257` (`get_provider`) ran.
**Change**: Added `ScoutSuite.providers.<cloud>.provider` for all seven providers to `hiddenimports`, mirroring the existing `authentication_strategy` block.
**Why**: `ScoutSuite/providers/__init__.py:12` does `__import__(f'ScoutSuite.providers.{provider}.provider', fromlist=[provider_class])` — the same dynamic-dispatch pattern as `authentication_strategy_factory.py:14`. PyInstaller's static analyzer cannot follow the f-string; `collect_submodules('ScoutSuite.providers.aws')` lists `provider` (verified locally) but PyInstaller silently drops the concrete submodule from the freeze. Explicit listing is the proven workaround.
**Real-creds verification**: Rebuilt `dist/scoutsuite.exe` on Windows, ran against a `cloudsaw` AWS CLI profile (account 928244370248, IAM ReadOnly):

```
INFO Launching Scout
INFO Authenticating to cloud provider
INFO Gathering data from APIs                  ← first time this line appears post-fix
INFO Fetching resources for the IAM service
INFO Running rule engine
INFO Saving data to scoutsuite_results_cloudsaw.js
exit: 0
```

Output file (2.1 MB) parsed cleanly by CloudSaw's `post_process_scoutsuite_output` regex; 37 real findings detected. End-to-end scan time: 19 seconds. **This is the first confirmed full-stack ScoutSuite scan from a CloudSaw-built binary against real AWS, ever.**
**What's still potentially fragile**: Other dispatch-style imports elsewhere in ScoutSuite that the fake-creds path didn't exercise but the real-creds path might. The CI E2E moto/localstack harness (open task #40) would catch any remaining gaps at build time instead of in production.
