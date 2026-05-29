# Windows Defender false-positive on `ScoutSuite/core/ruleset.py`

## Symptom you'll hit if this isn't configured

```
$ pyinstaller cloudsaw.spec
…
missing module named ScoutSuite.core.ruleset - imported by
  ScoutSuite.__main__ (top-level), …
…
# Then at scan time, inside CloudSaw:
ModuleNotFoundError: No module named 'ScoutSuite.core.ruleset'
[PYI-25852:ERROR] Failed to execute script 'scout' due to unhandled exception!
```

If you see that on a Windows developer machine, this doc is for you.

## What's actually happening

Windows Defender's ML-based real-time scanner flags
`vendor/scoutsuite/ScoutSuite/core/ruleset.py` as a generic
trojan (ThreatID 2147963851, `Trojan:Win32/Wacatac.B!ml` or
similar — the exact name depends on the signature build).

The triggering shape is the `TmpRuleset.__init__` block that
serializes a dict via `json.dumps`, writes it to a
`tempfile.TemporaryFile`, and reads it back. That sequence is
shared with real-malware "drop payload to temp then execute"
patterns; the ML model misfires on it.

The file content is unambiguously benign — `git log` shows it
traces straight to upstream ScoutSuite without CloudSaw-side
edits other than the `_cloudsaw_paths` import line. End users
running the signed release binary are not affected because the
module is embedded inside `scoutsuite.exe`, where Defender's
filesystem scanner can't reach it without unpacking the
executable.

## Fix (run once, as Administrator)

Open an **elevated** PowerShell (right-click → "Run as
administrator") and run:

```powershell
Add-MpPreference -ExclusionPath "C:\path\to\your\cloud-saw"
```

Replace the path with wherever you cloned the repo. Verify with:

```powershell
Get-MpPreference | Select-Object -ExpandProperty ExclusionPath |
  Where-Object { $_ -match 'cloud-saw' }
```

You should see the exclusion echoed back.

### Why a path exclusion specifically

A path exclusion only suppresses scanning for that location —
it does not lower Defender's protection for the rest of the
system, and it does not silence detections that originate from
processes running outside the excluded path. The scope is the
minimum that unblocks PyInstaller bundling.

## After applying the exclusion

```powershell
# Restore the quarantined file from git
cd C:\path\to\your\cloud-saw
git checkout HEAD -- vendor/scoutsuite/ScoutSuite/core/ruleset.py

# Confirm it actually sticks this time
Get-Item vendor\scoutsuite\ScoutSuite\core\ruleset.py
```

If `Get-Item` reports the file, you're good. Rebuild the
ScoutSuite bundle:

```bash
cd vendor/scoutsuite
python -m pip install -r pyinstaller-requirements.txt -r requirements.txt
pyinstaller cloudsaw.spec --distpath dist --workpath build --clean --noconfirm
```

A successful build prints `Building EXE from EXE-00.toc completed
successfully.` near the end. There should be no
`missing module named ScoutSuite.core.ruleset` warning in
`build/cloudsaw/warn-cloudsaw.txt`.

## Reverting the exclusion later

If you uninstall CloudSaw / delete the repo / change orgs and
want to clean up:

```powershell
Remove-MpPreference -ExclusionPath "C:\path\to\your\cloud-saw"
```

## Why we can't fix this in code

The triggering pattern lives in the upstream ScoutSuite source we
vendor. We can't refactor `TmpRuleset.__init__` without diverging
from upstream and inviting merge conflicts on the next vendor
refresh. We've filed defenses in two other layers instead:

  * `cloudsaw.spec` pre-declares every `ScoutSuite.core` /
    `ScoutSuite.output` / `ScoutSuite.providers` submodule via
    `collect_submodules` and asserts the result contains the
    static-import set from `ScoutSuite/__main__.py`. A missing
    source file now surfaces as a build-time error, not a
    runtime ModuleNotFoundError.
  * `.github/workflows/release.yml` runs the bundled binary's
    `--help` as a smoke test on every CI build and fails the
    build if `ModuleNotFoundError` ever lands in the output.

Together those mean a regression of this class can't ship a
broken binary to users via the release pipeline.
