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

The placeholder string `<MAINTAINER-FILLS-IN-AT-RELEASE-TIME>` is
shipped in `tauri.conf.json` until the maintainer generates a real
keypair (see below). Until then, the updater plugin will REJECT
every fetched update because no signature can verify against a
non-key — which is the safe default.

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

Contract 16 §Constraints requires that these are NOT plaintext CI
secrets. Two acceptable patterns:

1. **Local signing** — sign on the maintainer's workstation, attach
   the signed `latest.json` + binaries to the GitHub Release
   manually. The CI workflow builds the unsigned artifacts; the
   maintainer signs locally and attaches the signature.
2. **OIDC-gated** — store the secrets in a third-party secret store
   (HashiCorp Vault, AWS KMS) and have CI authenticate via OpenID
   Connect, never as a static long-lived token. The secret is
   retrieved at run time and never written to a workflow log.

For Phase 1 we use approach #1 (local signing). The workflow
publishes the unsigned binaries + checksums + SBOMs; the maintainer
signs and uploads `latest.json` separately.

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
| Public key fingerprint | `SECURITY.md` + `cloud-saw.com` | **Yes.** |
| Public key (ASCII-armored) | `cloud-saw.com/cloudsaw.asc`, mirrored to `keys.openpgp.org`. | Published. |
| Private key | Maintainer's password manager + offline backup. | **No.** |

Signing happens locally on the maintainer's workstation (same custody
pattern as approach #1 above) — `gpg --detach-sign --armor
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

- The Ed25519 updater **private** key is absent from this repo and
  from plaintext CI secrets. The repo grep below should return zero
  matches: `git grep -n "BEGIN PRIVATE KEY\|tauri-signature"`.
- The updater plugin verifies the signature before applying. The
  plugin source-level invariant is enforced by `tauri-plugin-updater`
  itself; the QA report references the upstream verification path.
- Every release publishes SHA-256 checksums, SLSA attestations, and
  CycloneDX SBOMs alongside the binaries — see
  `.github/workflows/release.yml` for the assembled outputs.
