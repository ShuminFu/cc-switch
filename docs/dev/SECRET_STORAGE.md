# Secret storage

CC Switch keeps the following credentials out of its plaintext JSON files:

| Secret | Previously stored in | Keychain entry (service `cc-switch`) |
| --- | --- | --- |
| WebDAV sync password | `~/.cc-switch/settings.json` → `webdavSync.password` | `settings.webdav.password` |
| S3 sync Secret Access Key | `~/.cc-switch/settings.json` → `s3Sync.secretAccessKey` | `settings.s3.secret_access_key` |
| GitHub Copilot OAuth token (per account) | `<app config dir>/copilot_auth.json` → `accounts.<id>.github_token` | `copilot.github_token.<id>` |
| Codex (ChatGPT) OAuth refresh token (per account) | `<app config dir>/codex_oauth_auth.json` → `accounts.<id>.refresh_token` | `codex_oauth.refresh_token.<id>` |

Provider API keys are **not** moved. The CLIs (Claude Code, Codex, Gemini CLI, …) read
them from their own live config files, so they stay where they are.

The code is in `src-tauri/src/secret_store.rs` (the `SecretStore` trait and `SecretPolicy`).

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
