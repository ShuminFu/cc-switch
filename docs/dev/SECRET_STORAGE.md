# Secret storage

CC Switch keeps the following credentials out of its plaintext JSON files:

| Secret | Previously stored in | Keychain entry (service `cc-switch`) |
| --- | --- | --- |
| WebDAV sync password | `~/.cc-switch/settings.json` → `webdavSync.password` | `settings.webdav.password` |
| S3 sync Secret Access Key | `~/.cc-switch/settings.json` → `s3Sync.secretAccessKey` | `settings.s3.secret_access_key` |
| GitHub Copilot OAuth token (per account) | `<app config dir>/copilot_auth.json` → `accounts.<id>.github_token` | `copilot.github_token.<id>` |
| Codex (ChatGPT) OAuth refresh token (per account) | `<app config dir>/codex_oauth_auth.json` → `accounts.<id>.refresh_token` | `codex_oauth.refresh_token.<id>` |
| WebDAV/S3 snapshot signing key (CCS-173) | never stored in a file | `settings.sync.snapshot_key` (PBKDF2-derived key, hex) |

Provider API keys are **not** moved. The CLIs (Claude Code, Codex, Gemini CLI, …) read
them from their own live config files, so they stay where they are.

The code is in `src-tauri/src/secret_store.rs` (the `SecretStore` trait and `SecretPolicy`).

## Sync snapshot signing key

Sync manifests (`manifest.json` on WebDAV/S3) carry an HMAC-SHA256 `signature`
over the other manifest fields, keyed with a sync key that the user enters on each
device (`set_sync_snapshot_key`). The key is PBKDF2-HMAC-SHA256 (600k rounds, fixed
salt) of the passphrase; only the derived key is stored. With a key configured:

- uploads are signed; downloads verify the signature before any artifact is fetched;
  a bad MAC (`sync.manifest_signature_invalid`) or another key
  (`sync.manifest_key_mismatch`) is always refused;
- an unsigned remote (uploaded by an older version) is refused with
  `sync.manifest_unsigned` unless the download is retried with `allowUnsigned: true`
  (the one-time migration confirmation);
- after the first signed upload/download (or the confirmed migration import) the
  transport's `status.signedSync` is set and unsigned remotes are refused for good
  (`sync.manifest_unsigned_rejected`); clearing the key resets it;
- after a signed download is applied, its `createdAt` is kept as the transport's
  `status.lastSignedManifestCreatedAt`, and a later signed manifest with an older
  `createdAt` is refused (`sync.manifest_rollback_rejected`), so a re-uploaded old
  snapshot signed with the same key cannot roll the device back. Equal or newer
  timestamps are accepted. Only devices that have applied a signed snapshot are
  protected (uploads do not set the floor), the check relies on the uploading
  devices' clocks, and clearing the key resets the floor.

Without a key and before any signed sync, sync behaves as before (unsigned).
The remote-info payload reports `signature`: `valid`, `invalid`, `unsigned` or `keyMissing`.

## Backends

1. **OS keychain** (default): macOS Keychain, Windows Credential Manager, or the
   freedesktop Secret Service (GNOME Keyring / KWallet) on Linux, via the `keyring` crate.
2. **File fallback**: `<config dir>/secrets.json`, written atomically and owner-only
   (0600 on Unix, owner-only ACL on Windows). It is used when the keychain is
   unavailable, for example on headless Linux without a Secret Service daemon or D-Bus
   session, or when `CC_SWITCH_SECRET_STORE=file` is set.

Test builds and processes with `CC_SWITCH_TEST_HOME` set always use the file backend inside
the test directory. They never touch the real keychain.

## Migration

Migration runs automatically and needs no user action:

- **Settings:** on startup, `settings::migrate_secrets()` writes any plaintext WebDAV or
  S3 secret into the store and rewrites `settings.json` with the field blanked (`""`).
- **Token stores:** when `CopilotAuthManager` or `CodexOAuthManager` loads its JSON file,
  plaintext tokens are moved into the store and the file is rewritten with blank
  token fields.

The JSON keeps every other field, so the files stay valid for older versions. If a
keychain write fails, the secret stays in plaintext in the 0600 file and nothing is
lost. The next save tries again. If a keychain read fails (for example the keychain is
locked or access was denied), the secret is treated as missing for that run but is not
deleted from the keychain by later saves.

## Choosing a backend: `CC_SWITCH_SECRET_STORE`

| Value | Behaviour |
| --- | --- |
| unset / `auto` / `keychain` | Use the keychain if it works, otherwise fall back to `secrets.json`. |
| `file` | Always use `secrets.json`. |
| `plaintext` | Downgrade mode (see below). Write the secrets back into `settings.json` and the token files, then remove them from the store. |

## Downgrade path

Older versions only read secrets inline. After a downgrade they would see blank WebDAV/S3
passwords and blank OAuth tokens. Sync would then fail authentication, and Copilot/Codex
accounts would ask you to log in again. To downgrade without re-entering anything:

1. Quit CC Switch.
2. Start the **current** version once with `CC_SWITCH_SECRET_STORE=plaintext`:
   - macOS: `CC_SWITCH_SECRET_STORE=plaintext /Applications/CC\ Switch.app/Contents/MacOS/cc-switch`
   - Linux: `CC_SWITCH_SECRET_STORE=plaintext cc-switch` (or the AppImage path)
   - Windows (PowerShell): `$env:CC_SWITCH_SECRET_STORE="plaintext"; & "C:\Program Files\CC Switch\cc-switch.exe"`
3. Wait for the main window to appear. The Copilot and Codex OAuth token stores load at
   startup, and loading them rewrites the token files with the tokens inline.
4. Quit, then install the older version.

If you skip these steps, re-enter the WebDAV/S3 password in **Settings → Sync** and log in to
Copilot/Codex again in the older version. The keychain entries can also be inspected or deleted by
hand:

- macOS: Keychain Access, search for `cc-switch`.
- Windows: Credential Manager → Windows Credentials, entries named after the keys above.
- Linux: Seahorse (Passwords and Keys), or `secret-tool search service cc-switch`.

## Manual verification checklist

Run this on each of macOS, Windows and Linux (desktop, plus headless Linux for the fallback):

1. Configure WebDAV sync with a password and log in to a Copilot account.
2. Restart the app. `settings.json` → `webdavSync.password` and
   `copilot_auth.json` → `github_token` are `""`. The keychain shows entries under
   service `cc-switch`. Sync and Copilot requests still work.
3. Remove the Copilot account. Its keychain entry disappears.
4. Headless Linux (no D-Bus session): secrets land in `~/.cc-switch/secrets.json` (mode 0600).
5. Run the downgrade path above. The plaintext values reappear in the JSON files and the
   keychain entries are removed.

### Linux Secret Service (automated)

`scripts/check-linux-secret-service.sh` covers steps 2-5 at the API level for the
freedesktop Secret Service (GNOME Keyring). It needs `dbus`, `gnome-keyring` and
`libsecret-tools` (`secret-tool`). It starts a private D-Bus session with an unlocked
gnome-keyring in a temporary data directory (the login keyring is not touched) and runs
the ignored `secret_store::keyring_live` tests under the service `cc-switch-test-<pid>`:

- probe, set/get/delete round trip;
- `SecretPolicy` externalize/hydrate of the WebDAV password and a Copilot token, `forget()`;
- `plaintext` mode: the value is written back inline and `release()` deletes the entry;
- an entry deleted outside the app (Seahorse, `secret-tool clear`) still reads from the
  process cache until restart; a fresh process sees it gone.
- interop with an independent client: an entry stored with `secret-tool store` is read by
  the app, and the value the app writes back is confirmed with `secret-tool lookup`.

It then checks that `secret-tool search --all service cc-switch-test-<pid>` finds nothing,
and runs `probe_fails_without_dbus` with no session bus (`DBUS_SESSION_BUS_ADDRESS` unset,
`XDG_RUNTIME_DIR` pointed at an empty directory): the probe fails and the secret lands in a
0600 `secrets.json`. Last run: passed on Ubuntu 24.04 with gnome-keyring 46.1 (CCS-240).
KWallet and interactive unlock prompts are not covered.
