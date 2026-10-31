# Release Signing Key Management

Runbook for the keys and credentials the release workflow (`.github/workflows/release.yml`)
uses (CI-36). Keep this page current whenever a secret is added, rotated or removed. All
secrets live in the **`release` GitHub environment** (Settings → Environments → `release`),
which should require a maintainer's approval.

## Inventory

| Key / credential | Secret names | Used for | If lost | If leaked |
| --- | --- | --- | --- | --- |
| **Updater signing key** (minisign) | `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Signs every updater artifact; installed apps verify against the public key in `src-tauri/tauri.conf.json` (`plugins.updater.pubkey`) | **Installed apps can never auto-update again.** Users must download a new build manually | An attacker who also controls the download URL can push an "update" to every user |
| Apple Developer ID certificate | `APPLE_CERTIFICATE` (base64 `.p12`), `APPLE_CERTIFICATE_PASSWORD`, `KEYCHAIN_PASSWORD` | Code-signs the macOS app and DMG | Re-issue from the Apple Developer account; no user impact | Revoke in the Apple Developer account immediately; Apple's revocation also stops Gatekeeper accepting software signed with it |
| Notarization: App Store Connect API key (preferred) | `APPLE_API_KEY_ID`, `APPLE_API_ISSUER`, `APPLE_API_KEY_P8` | `xcrun notarytool` for the app and DMG (`scripts/ci/notarize-macos.sh`) | Create a new key | Revoke it in App Store Connect → Users and Access → Integrations |
| Notarization: Apple ID (legacy fallback) | `APPLE_ID`, `APPLE_PASSWORD` (app-specific password), `APPLE_TEAM_ID` | Same, used only when the API key secrets are absent | Generate a new app-specific password | Revoke the app-specific password at appleid.apple.com |
| Windows Authenticode certificate (optional) | `WINDOWS_CERTIFICATE` (base64 `.pfx`), `WINDOWS_CERTIFICATE_PASSWORD` | Signs `cc-switch.exe` and the MSI (CI-01). Without it Windows assets ship unsigned | Re-issue from the CA; no user impact | Ask the CA to revoke it |
| `GITHUB_TOKEN` | automatic | Creating the release; attestations (`id-token: write`) | — | Scoped to the run |

## Custody

- At least **two maintainers** hold an offline copy of the updater private key and its
  password (password manager entry plus an encrypted offline backup). Record who in the
  maintainers' private notes, not in this repository.
- The minisign key file is stored as-is: the two-line file that starts with
  `untrusted comment:`, base64-encoded as one line, is what `tauri signer generate` produces and
  what the workflow expects. The "Prepare Tauri signing key" step also accepts the raw two-line
  file and the bare key line for compatibility; when the secret is next re-entered, store the
  base64 form and the other branches can be deleted.
- Never paste a key into an issue, PR, chat or log. The workflow writes keys only to
  `$RUNNER_TEMP` with `umask 077`, masks them, and deletes them in the final step.

## Routine rotation

| What | When | How |
| --- | --- | --- |
| Apple app-specific password | Yearly, or when the person who owns the Apple ID leaves | Prefer replacing it with an App Store Connect API key (team-owned), then delete `APPLE_ID` / `APPLE_PASSWORD` |
| App Store Connect API key | Yearly | Create a new key with the Developer role, update the three secrets, run a signed dry run (`workflow_dispatch`, `sign: true`), revoke the old key |
| Apple / Windows certificates | Before expiry (the workflow logs the Windows certificate's expiry date) | Re-issue, update the secrets, signed dry run |
| Updater key | Only when compromised or when a stronger scheme is adopted | See below; it needs a release in between |

## Rotating the updater key

Tauri's updater trusts exactly **one** public key, the one compiled into the installed app. A new
key therefore has to reach users inside an update signed with the **old** key:

1. Generate a new key pair: `pnpm tauri signer generate -w cc-switch-updater-v2.key`.
2. Release N: put the new public key into `src-tauri/tauri.conf.json` and ship the release
   **signed with the old key** (secrets unchanged). Installed apps verify it with the old key
   they trust, install it, and from then on trust only the new key.
3. Switch the `TAURI_SIGNING_PRIVATE_KEY[_PASSWORD]` secrets to the new key.
4. Release N+1 onwards is signed with the new key.
5. Users who skip release N cannot auto-update past it; say so in the release notes of N+1 and
   keep N downloadable.

## Incident: updater key leaked

1. Assume an attacker can sign updates. Rotate immediately following the steps above, with
   release N as an emergency patch release.
2. Publish a GitHub security advisory telling users to update to N manually from the Releases
   page and to verify `SHA256SUMS` and the provenance attestation
   ([SECURITY.md](../../SECURITY.md#verifying-a-download)).
3. Review recent releases' `latest.json` and attestations for artifacts you did not build.

## Incident: updater key lost

1. Installed apps can no longer auto-update. Generate a new key, put its public key into
   `tauri.conf.json` and release normally.
2. Tell users in the release notes, the README and a pinned issue that this one update must be
   installed manually; after that, auto-update works again.

## Tabletop check

Once a year, a second maintainer walks through "updater key lost" and "updater key leaked"
using only this page and [RELEASING.md](../../RELEASING.md), and fixes whatever was unclear.
