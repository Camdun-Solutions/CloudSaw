# ScoutSuite hardened-runtime entitlements

Why `entitlements.plist` exists, in long-form. (The plist itself can't
hold a comment this long because XML comments cannot contain the
two-dash sequence used in CLI flag names like `pyinstaller`'s onefile
mode, so the explanation lives here instead.)

## The failure mode this fixes

PyInstaller's onefile mode bootstraps by extracting an embedded
`Python.framework` (and its associated `.dylib`s) into a temp directory
at launch, then `dlopen()`'ing the framework from there. The outer
`scoutsuite` Mach-O is codesigned with CloudSaw's Apple Developer ID
team in `release.yml`. The inner `Python.framework` was built upstream
(python.org / actions/setup-python) and carries a *different* Team ID
baked into its own code signature.

macOS's hardened runtime defaults to library-validation enforcement: it
refuses to load a dylib whose Team ID does not match the host process.
The result is a `dlopen` failure at scan-time:

```
code signature ... not valid for use in process: mapping
process and mapped file (non-platform) have different Team IDs
```

ScoutSuite dies before it can import any of its own modules, exits
non-zero, and CloudSaw maps the non-zero exit to a generic
"scanner process failed" message — leaving the user with no useful
signal until they dig into `scoutsuite-stderr.log`.

This is the failure that shipped in 2026.5.9 through 2026.5.12 on
macOS. The argv fix in 2026.5.12 was correct but masked by this
deeper signing issue.

## Why these specific entitlements

The supported Apple workaround for PyInstaller-frozen apps is to
weaken library-validation for *this* process only:

- **`com.apple.security.cs.disable-library-validation`** — the
  primary fix. Tells `dyld` to skip the Team-ID match check when
  loading libraries into this process. Required by every notarized
  PyInstaller onefile app; documented at
  <https://pyinstaller.org/en/stable/feature-notes.html#macos> and
  <https://developer.apple.com/documentation/bundleresources/entitlements/com_apple_security_cs_disable-library-validation>.

- **`com.apple.security.cs.allow-unsigned-executable-memory`** —
  PyInstaller's bootstrap maps the extracted Python framework into
  executable memory pages that aren't signed. Without this entitlement
  the hardened runtime kills the process at `mmap` time.

- **`com.apple.security.cs.allow-dyld-environment-variables`** —
  PyInstaller relies on `DYLD_LIBRARY_PATH`-style environment
  variables to point its bootstrap at the extracted framework.
  Hardened runtime strips these by default; this entitlement lets
  them through.

## Blast-radius

These entitlements are scoped to `scoutsuite` ONLY. The outer
CloudSaw `.app` keeps the full hardened-runtime defaults — only the
inner Python interpreter gets the weakening, which is correct: any
RCE vector in ScoutSuite stays sandboxed to the scoutsuite child
process and cannot escalate into CloudSaw itself.

## Verifying the fix landed

After install on a macOS host, run:

```
codesign --display --entitlements - \
    /Applications/CloudSaw.app/Contents/Resources/vendor/scoutsuite/aarch64-apple-darwin/scoutsuite
```

The output should include all three entitlement keys above set to
`true`. The release workflow's `codesign bundled scoutsuite (macOS)`
step asserts this with a `grep` guard so a regression surfaces in
CI rather than at runtime on a user's machine.
