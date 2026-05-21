# Contributing to CloudSaw

CloudSaw is built one **contract** at a time. Each contract is a single
self-contained unit of work — a feature, its tests, and its acceptance
criteria — and lives on its own branch. Read `CLAUDE.md` before starting,
then the contract you intend to implement.

## Branching

  - `master` is protected. Direct pushes are rejected. (Renaming to `main`
    is under consideration; see issue tracker.)
  - Every contract lives on `feature/<contract-number>-<short-slug>`, branched
    from `master` (e.g. `feature/01-foundation`, `feature/02-app-lock`).
  - Commits must be **signed** (`git commit -S`). Unsigned commits will be
    rejected by branch protection.
  - `Cargo.lock` and `package-lock.json` are committed.

## Branch protection (configured in GitHub settings)

  - Require pull requests before merging to `master`.
  - Require at least one approving review.
  - Require status checks to pass: `ci / build`, `ci / lint`,
    `ci / test`.
  - Require signed commits.
  - Require linear history (no merge commits on `master`).
  - Restrict who can push directly to `master` (default: none).
  - Restrict force-pushes and branch deletion.

## Pull requests

A PR is mergeable only when:

  1. Its paired QA & Security Verification contract has passed.
  2. The verification summary is pasted into the PR description.
  3. All CI checks are green.
  4. A human has reviewed and approved.

Auto-merge for dependency updates is **disabled** (see CLAUDE.md §5).

## Architecture rules of thumb

These are short reminders; the authoritative source is `CLAUDE.md`.

  - The frontend is untrusted UI. Every privileged action goes through Rust.
  - Every component talks to the backend through `src/lib/ipc.ts` —
    no direct `invoke()` calls in components, hooks, or routes.
  - Every Rust public function returns `Result<T, AppError>`.
  - No hardcoded user-facing strings — all strings flow through the i18n hook.
  - No `localStorage` / `sessionStorage` / browser storage anywhere.
  - No credentials, tokens, or API keys in SQLite, config, logs, or URLs.
  - External binaries are invoked by absolute path with argv arrays — never
    through a shell, never with interpolated strings.
  - GitHub Actions are pinned to full commit SHAs, never floating tags.

## Local dev setup

```bash
rustup default stable
# Install Node 20+ LTS
npm install
npm run tauri dev
```

Tests:

```bash
npm run lint           # TypeScript typecheck
cargo test --manifest-path src-tauri/Cargo.toml
```

## Reporting bugs

  - **Security issues:** see [`SECURITY.md`](SECURITY.md) — do **not** open
    public issues for vulnerabilities.
  - **Everything else:** GitHub Issues, with reproduction steps and the
    `Help → Report a problem` payload from inside the app where applicable.
