# CloudSaw

> Local-first AWS security scanner. Runs entirely on your machine. No
> CloudSaw-hosted infrastructure. No telemetry. No data leaves your laptop.

CloudSaw scans AWS environments for security misconfigurations:

  - **Local-first.** Findings, scan history, and settings live in SQLite on
    your machine. The app never phones home and has no analytics.
  - **AWS-native auth.** Uses the AWS SDK credential provider chain (profiles
    or IAM Identity Center / SSO). No long-term secret access keys are ever
    accepted in the UI.
  - **Bundled toolchain.** Ships with hash-verified copies of
    [ScoutSuite](https://github.com/nccgroup/ScoutSuite) and
    [Terraform](https://terraform.io) — no separate installs required.
  - **Compliance-mapped explanations.** Each finding links to a bundled
    markdown knowledge-base article and the compliance controls it touches.

CloudSaw is **Apache 2.0** licensed. See [`LICENSE`](LICENSE) and
[`NOTICE`](NOTICE).

## Status

🚧 **Pre-launch — Contract 01 (Foundation & Scaffold).** The repo currently
boots an empty Tauri 2 shell and verifies the IPC + SQLite migration
plumbing. Feature contracts will follow on dedicated branches.

## Repository layout

```
.
├── src/             React 18 + TypeScript + Tailwind frontend (untrusted UI)
├── src-tauri/       Rust backend (all privileged ops live here)
│   ├── src/         auth, terraform, scanner, findings, knowledgebase,
│   │                reports, ipc, errors, db
│   └── migrations/  Forward-only SQLite schema migrations
├── vendor/
│   └── scoutsuite/  Mirrored ScoutSuite source (separate-process aggregation)
└── .github/         CI workflows (Actions pinned to commit SHAs)
```

## Building from source

You need:

  - **Rust** stable (rustup-managed)
  - **Node.js** 20+ LTS
  - **Tauri 2 prerequisites** for your OS — see
    https://v2.tauri.app/start/prerequisites/

Then:

```bash
npm install
npm run tauri dev      # development build, live-reloaded
npm run tauri build    # production bundle
```

## Versioning

CloudSaw uses **CalVer** in `YYYY.MM.PATCH` form (e.g. `2026.5.0`).

## Distribution

Binaries are published on [GitHub Releases](https://github.com/Camdun-Solutions/CloudSaw/releases).
The macOS build is Apple-Developer-ID-signed and notarized; the Linux build
ships with detached GPG signatures; the Windows build is currently
unsigned (a signed build via EV certificate is on the roadmap).

## Security

See [`SECURITY.md`](SECURITY.md) for the disclosure policy and how to
contact us privately about a vulnerability.

## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md).
