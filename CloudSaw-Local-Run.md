# Running CloudSaw locally from source

Contract 16F — step-by-step instructions for running CloudSaw on
macOS, Windows, and Linux without installing a release build. The
"Local Run" path compiles the desktop app from this repo and launches
it through Tauri's dev runtime, giving the exact same behavior the
shipped binary delivers minus the platform code-signing.

This document is tested on every platform during the Contract 16 QA
pass. If any step is wrong on your system, please file a bug report
at `https://github.com/Camdun-Solutions/CloudSaw/issues` and include
the platform + error output.

---

## 0. Common prerequisites (all platforms)

* **Rust 1.77 or newer**, installed via [`rustup`](https://rustup.rs/).
  Verify with `rustc --version`.
* **Node.js 20+ LTS** and **npm 10+**. Verify with `node --version`
  and `npm --version`. nvm/fnm are fine.
* **Git** — any modern version.
* About **8 GB** of free disk space (the AWS SDK + Tauri toolchain
  is heavy on first build).

Then, in any platform-specific terminal:

```sh
git clone https://github.com/Camdun-Solutions/CloudSaw.git
cd CloudSaw
npm ci
```

---

## 1. macOS

### Prerequisites
* **Xcode Command Line Tools**: `xcode-select --install`.
* That's it. macOS ships the WebKit framework Tauri needs.

### Run from source (dev mode)
```sh
npm run tauri dev
```

The Tauri runtime starts Vite on `http://localhost:1420` and
launches the desktop window once the bundle is ready. First run
takes 5–10 minutes while cargo compiles the dependency graph;
subsequent runs are seconds.

### Build a release bundle locally
```sh
npm run tauri build -- --bundles dmg
```
Output: `src-tauri/target/release/bundle/dmg/CloudSaw_*.dmg`. Local
builds are NOT signed by Apple Developer ID; the CI release
workflow is what produces the notarized DMG (see
`.github/workflows/release.yml`).

### Verify the dev run
1. The app window opens with the onboarding wizard.
2. Step 1 (language) shows a Select with English / Español /
   Français / 中文.
3. Quit the app and re-launch — the wizard resumes at the step
   you were on (Contract 14 §Edge Cases).

---

## 2. Windows 10 / 11

### Prerequisites
* **Microsoft Edge WebView2 runtime** — pre-installed on Windows 11,
  recent Windows 10. Verify in *Settings → Apps* (search "WebView2").
  Install from
  [Microsoft's evergreen installer](https://developer.microsoft.com/en-us/microsoft-edge/webview2/)
  if missing.
* **Microsoft C++ Build Tools** — the C++ workload of
  [Visual Studio Build Tools 2022](https://visualstudio.microsoft.com/visual-cpp-build-tools/).
  Install at least: *MSVC v143 (or current)*, *Windows SDK*, and
  *C++ build tools*. ~6 GB.
* **Git for Windows** — provides `git`, `bash`, and a usable terminal
  (Git Bash). The instructions below use PowerShell, but Git Bash
  works equivalently.

### Run from source (dev mode)
Open **PowerShell** in the repo root:
```powershell
npm run tauri dev
```
On first build cargo will compile the entire dependency graph
(~10-15 min on a typical laptop SSD). Subsequent runs are 5-15
seconds.

If the build complains about `link.exe` or `cl.exe` not being
found, your Visual Studio Build Tools install is incomplete —
re-run the installer with the *C++ build tools* workload checked.

### Build an installer locally
```powershell
npm run tauri build -- --bundles nsis
```
Output: `src-tauri/target/release/bundle/nsis/CloudSaw_*-setup.exe`.
**Windows builds are unsigned** at this stage (CLAUDE.md §6.2 +
Contract 16 §Constraints). You'll see a SmartScreen warning when
running the installer; that's expected. See the install guide on
cloud-saw.com for the manual verification path.

### Verify the dev run
1. Onboarding window opens; the title bar reads "CloudSaw".
2. Step 1 (language) renders the language picker.
3. CloudSaw stores its data root at `%APPDATA%\CloudSaw\`.
   Confirm in *File Explorer* that the directory appears after
   the first step. The directory inherits the user-only ACL of
   `%APPDATA%`.

### Reset for a clean re-test
```powershell
Remove-Item -Recurse -Force "$env:APPDATA\CloudSaw"
```

---

## 3. Linux (Ubuntu / Debian / Fedora / Arch)

### Prerequisites (Ubuntu 22.04+, Debian 12+)
```sh
sudo apt-get update
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev \
  libssl-dev libgtk-3-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  build-essential curl wget file libxdo-dev pkg-config
```

### Prerequisites (Fedora 39+)
```sh
sudo dnf install -y \
  webkit2gtk4.1-devel openssl-devel gtk3-devel \
  libappindicator-gtk3-devel librsvg2-devel \
  curl wget file libxdo-devel pkgconf-pkg-config @development-tools
```

### Prerequisites (Arch / Manjaro)
```sh
sudo pacman -S --needed \
  webkit2gtk-4.1 openssl gtk3 libayatana-appindicator librsvg \
  curl wget file xdotool base-devel pkgconf
```

### Run from source (dev mode)
```sh
npm run tauri dev
```

### Build a release bundle locally
```sh
npm run tauri build -- --bundles appimage
# or
npm run tauri build -- --bundles deb
```
Outputs at `src-tauri/target/release/bundle/{appimage,deb}/`. Local
builds are NOT GPG-signed; signed signatures come from the
maintainer's release workflow (see `docs/release-signing.md`).

### Verify the dev run
1. Onboarding window opens.
2. Step 1 (language) renders the language picker.
3. Data root is `~/.local/share/cloudsaw/` (CLAUDE.md §6.7).
   Confirm with `ls -la ~/.local/share/cloudsaw/`.

### Reset for a clean re-test
```sh
rm -rf ~/.local/share/cloudsaw
```

---

## 4. Common dev-mode tasks

### Run the Rust test suite
```sh
cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast
```
On a fresh clone this takes 5–15 minutes. The QA contracts for
every feature contract (C05, C06, C10, C11, C12, C13, C14, C15,
C16) run from here.

### TypeScript / Vite lint
```sh
npm run lint    # tsc --noEmit
npm run build   # tsc --noEmit && vite build
```

### Switch language at runtime
Open Settings → AI section → onboarding language picker, or open
the in-app `__cloudsaw_dev.setLocale("es")` developer hook from
the browser console (DEV builds only — stripped in release).

### Hot-reload tips
* React + Vite hot-reload works in `npm run tauri dev` — edits to
  `src/**.tsx` reload instantly.
* Edits to Rust files trigger a `cargo` rebuild and a window
  refresh (~5-15s).
* If the IPC bridge wedges, fully quit the Tauri window (Ctrl+Q /
  Cmd+Q) and re-run `npm run tauri dev`.

---

## 5. Troubleshooting

* **"failed to build … libwebkit2gtk-4.1-dev"** on Ubuntu — older
  releases ship 4.0; install 22.04 LTS or newer, or run the
  fallback `sudo apt-get install libwebkit2gtk-4.0-dev` (functional
  but with a deprecation warning).
* **First build is slow** — that's `aws-sdk-rust` and `printpdf`
  compiling. The cargo cache (`~/.cargo/registry`) is reusable
  across projects, so the second run on the same machine is fast.
* **"WebView2 missing"** on Windows — install the runtime from the
  link above and re-launch.
* **"Permission denied" on `~/.local/share/cloudsaw`** — the data
  root inherits 0700 on Unix (CLAUDE.md §4.5). If you run the dev
  build as a different user, delete the directory and let the next
  launch recreate it under your own user.

---

## 6. What this dev build does NOT include

* **Bundled Terraform / ScoutSuite binaries.** The dev build skips
  the binary bundling step that the release workflow performs.
  Contract 05 + 06 features that depend on those binaries (the
  scanner-role provisioner and the scan engine) won't execute end-
  to-end without them. Drop a verified Terraform binary at
  `src-tauri/binaries/terraform/<target-triple>/terraform` and the
  ScoutSuite binary at `src-tauri/binaries/scoutsuite/<target-triple>/scoutsuite`
  to exercise the full pipeline.
* **Signed binaries.** Dev builds are unsigned across all
  platforms. The release workflow (Contract 16A) is the only path
  that produces signed/notarized macOS DMGs and GPG-signed Linux
  artifacts.
* **Real auto-updater verification.** The dev build still calls
  the update endpoint and verifies the Ed25519 signature on
  `latest.json` against the configured `tauri.conf.json` `pubkey`
  (minisign `1A6CC676BC0CFA2E`). A dev build will only accept an
  update whose signature was produced by the matching private key,
  which lives offline in the maintainer's custody — so locally
  running `tauri build` and publishing an unsigned `latest.json`
  will (correctly) be rejected.
