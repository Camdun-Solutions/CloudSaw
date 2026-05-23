# Release signing & key custody

This document describes the three signing keys the CloudSaw release
pipeline uses, how they are stored, and how they are rotated. The
**Ed25519 updater key is the single highest-value secret in the
project** — it gates whether installed copies will accept an update.

> Read [CLAUDE.md](../CLAUDE.md) §4.3 and §6.3 before changing any of
> the workflows or this doc.

---

## 1. Tauri Ed25519 updater key (Contract 16C)

The Tauri updater plugin (`tauri-plugin-updater`) verifies an
Ed25519 signature on every `latest.json` manifest fetched from the
configured endpoint **before** any update bytes are applied. An
unsigned, mis-signed, or expired-keypair signature is rejected and
the user is informed; the running app is never replaced silently.

### Where the keys live

| Component | Storage | Repo? |
|---|---|---|
| Public key (verifier) | `src-tauri/tauri.conf.json`, `plugins.updater.pubkey` | **Yes** — checked into source. |
| Private key (signer) | Maintainer's password manager + offline backup (encrypted USB + paper recovery code). Never on a build machine. | **No.** |
| Private-key password | Same password manager entry as the key itself. | **No.** |

The current verifier key is the minisign public key with the short
identifier `1A6CC676BC0CFA2E` (the value in `tauri.conf.json` is the
base64-encoded form of the `.pub` file, which `tauri-plugin-updater`
decodes at runtime). If you ever see `<MAINTAINER-FILLS-IN-AT-RELEASE-TIME>`
back in this field, the placeholder has been re-introduced — the
updater will REJECT every fetched update in that state, which is the
safe default. The setup procedure below produced the current key.

### Generating the keypair (one-time setup, then rotation)

Run **locally** on the maintainer's workstation:

```sh
npm run tauri signer generate -- --write-keys ~/.cloudsaw-signing/
```

This emits:

- `~/.cloudsaw-signing/cloudsaw.key` — the private key, ASCII-armored,
  password-protected. **Move this to your password manager and the
  offline backup. Do NOT commit it. Do NOT email it. Do NOT paste it
  into a chat.**
- `~/.cloudsaw-signing/cloudsaw.key.pub` — the public key.

Copy the contents of `cloudsaw.key.pub` (a base64 blob starting with
`untrusted comment:`-ish marker followed by the key) into
`src-tauri/tauri.conf.json` at `plugins.updater.pubkey`. Open a PR;
the public key is OK to live in source.

### Signing a release

The release workflow (`.github/workflows/release.yml`) signs the
generated update bundles by invoking `tauri build` with the
`TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`
environment variables set. **Both are GitHub Actions secrets
provisioned by the maintainer through the repo settings**:

- `TAURI_SIGNING_PRIVATE_KEY` — the contents of the private-key file.
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` — the password.

The private key must never be committed, never written to a workflow
log, and never persisted to a runner's filesystem. Three custody
patterns satisfy that bar with increasing levels of hardening:

1. **Local signing** — sign on the maintainer's workstation, attach
   the signed `latest.json` + binaries to the GitHub Release
   manually. The CI workflow builds the unsigned artifacts; the
   maintainer signs locally and attaches the signature. The private
   key never enters CI.
2. **GitHub Actions encrypted secrets** — store the private key as
   the `TAURI_SIGNING_PRIVATE_KEY` repository secret (paired with
   `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`). The release workflow maps
   both into the `tauri build` step's process environment for the
   duration of that step only; CI emits signed updater artifacts
   directly. Secrets are encrypted at rest in GitHub's secret store
   and scrubbed from workflow logs by GitHub Actions' built-in
   masking.
3. **OIDC-gated external vault** — store the secrets in a third-
   party secret store (HashiCorp Vault, AWS KMS) and have CI
   authenticate via OpenID Connect, never as a static long-lived
   token. The secret is retrieved at run time and never written to
   a workflow log.

**Phase 1 (current): approach #2.** The release workflow at
`.github/workflows/release.yml` maps the two secrets into the
`tauri build` step's `env:` block, so signed `latest.json` +
updater artifacts come straight out of CI. The QA test
`security_release_workflow_loads_updater_private_key_from_secrets_only`
asserts the env block can only reach the key via the encrypted
secret reference (i.e. no inline literal). The trade-off vs
approach #3 is that the secret is decrypted into the runner's
process environment for the duration of `tauri build`; we mitigate
by (a) scoping the env mapping to that single step (not the whole
job), (b) restricting the workflow to tag-triggered runs, and (c)
not accepting `pull_request` triggers on this workflow so untrusted
code never runs in the same job.

A future hardening pass moves the secrets to approach #3 (OIDC-gated
external vault) and drops the long-lived repo-stored secret entirely.

### Key rotation

If the private key is suspected compromised:

1. Generate a new keypair (same command).
2. Update `pubkey` in `tauri.conf.json`.
3. Release a new version. Older installs ARE still trusted (their
   embedded pubkey verifies older updates), but they will reject
   any update signed with the new key — the user must re-install
   manually from the website.
4. Communicate the rotation via a SECURITY advisory.

There is no in-app key rotation — the pubkey is compiled into the
binary.

---

## 2. PGP key for Linux artifact signing (Next Steps A2)

The Linux `.AppImage` and `.deb` artifacts are accompanied by a
detached GPG signature so users can verify them with a single
command.

| Component | Storage | Repo? |
|---|---|---|
| Public key fingerprint | `SECURITY.md` + `cloud-saw.com`. Current fingerprint: `7CBC 9415 96B1 C393 6593 8A5E D932 48B2 4ADA 9EA4` (long key ID `D932 48B2 4ADA 9EA4`). | **Yes.** |
| Public key (ASCII-armored) | `cloud-saw.com/cloudsaw.asc`, mirrored to `keys.openpgp.org`. | Published. |
| Private key | Maintainer's password manager + offline backup. | **No.** |

Signing happens locally on the maintainer's workstation (same custody
pattern as approach #1 above for the Tauri updater key) — `gpg --detach-sign --armor
cloudsaw_*_amd64.AppImage`. The resulting `.asc` files are attached
to the GitHub Release alongside the artifacts and the SHA-256
checksums.

---

## 3. Apple Developer ID (Next Steps A1)

macOS `.dmg` artifacts are signed with the Apple Developer ID
certificate and notarized by Apple.

| Component | Storage | Repo? |
|---|---|---|
| Certificate (`.p12`) | Maintainer's local Keychain. | **No.** |
| App-specific password | Apple ID. Held in the maintainer's password manager. | **No.** |

The release workflow exports the certificate to the GitHub Actions
macOS runner via the `APPLE_CERTIFICATE` secret (base64-encoded
`.p12`) and the runner's keychain. Notarization is invoked via
`notarytool` with the app-specific password (`APPLE_PASSWORD` secret)
and the team ID (`APPLE_TEAM_ID` secret).

---

## Acceptance assertions

Contract 16 §Security Check + §Acceptance Criteria require:

- The Ed25519 updater **private** key is absent from this repo
  (never committed as a literal, even in encrypted form). It lives
  only inside GitHub's encrypted secret store as
  `TAURI_SIGNING_PRIVATE_KEY`. The repo grep below should return
  zero matches: `git grep -n "BEGIN PRIVATE KEY\|tauri-signature"`.
  The release workflow MUST source the key through the
  `${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}` reference — never as a
  pasted literal. The QA test
  `security_release_workflow_loads_updater_private_key_from_secrets_only`
  enforces both halves.
- The updater plugin verifies the signature before applying. The
  plugin source-level invariant is enforced by `tauri-plugin-updater`
  itself; the QA report references the upstream verification path.
- Every release publishes SHA-256 checksums, SLSA attestations, and
  CycloneDX SBOMs alongside the binaries — see
  `.github/workflows/release.yml` for the assembled outputs.
