# Secrets at rest and in sync

This page covers how CC Switch protects credentials on disk and in cloud sync, what is
already in place, and how the opt-in provider API-key vault works
(backlog items SEC-05, SEC-06, SVC-04, CMD-04, DB-02, PRODUCT-01).

## Threat model

| Threat                                                                                 | Covered by                                                                                                                                                                                                            |
| -------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Another local account reads `~/.cc-switch` (shared Linux box, permissive umask)        | Owner-only permissions (Unix)                                                                                                                                                                                         |
| A WebDAV/S3 operator, a shared or leaked bucket, or a weak NAS reads the sync snapshot | End-to-end sync encryption (opt-in)                                                                                                                                                                                   |
| Someone tampers with the remote snapshot                                               | AEAD authentication of every encrypted artifact                                                                                                                                                                       |
| `settings.json` is copied by a backup or dotfile sync                                  | WebDAV password, S3 secret and sync passphrase are kept in the OS keychain                                                                                                                                            |
| Malware running as the same user                                                       | **Not covered.** Same-user processes can read the keychain items this app can read, and the CLI configs that CC Switch writes (`~/.claude/settings.json`, `~/.codex/auth.json`, ...) hold keys in plaintext by design |

## 1. File permissions (SEC-05)

On Unix:

- `~/.cc-switch` (or the configured app config directory) is created, or tightened, to `0700`.
  The home directory itself is never changed.
- `cc-switch.db` is created `0600` before SQLite opens it; SQLite gives `-wal`/`-shm` the same
  mode. An existing database with wider permissions is tightened at startup.
- Database backups in `backups/` are created `0600`, and that directory is `0700`.
- SQL exports are written with `atomic_write_secret` (new files `0600`; an existing target
  keeps its mode).
- `settings.json`, live config files and logs were already owner-only (wave 1).

Helpers: `config::ensure_private_dir`, `config::prepare_private_file`,
`config::restrict_file_to_owner`.

**Windows:** ACLs are left as they are. Files under `%USERPROFILE%\.cc-switch` inherit the
profile's ACL, which by default grants access to the user, `SYSTEM` and Administrators only.
Exports saved elsewhere inherit the ACL of the chosen folder. Explicit DACL hardening
(`SetNamedSecurityInfoW`) is not implemented.

## 2. End-to-end sync encryption (SEC-06, SVC-04)

Opt-in, in **Settings → Sync → End-to-end encryption**. One passphrase covers both WebDAV and S3.

- **KDF:** Argon2id, 64 MiB, 3 passes, 1 lane, 16-byte random salt, 32-byte key.
- **Cipher:** XChaCha20-Poly1305 with a random 24-byte nonce for each artifact.
- **Envelope (version 1):** `"CCSENC\0" + 0x01 | nonce (24) | ciphertext + tag`. The AAD is
  the magic followed by the artifact name, so `db.sql` and `skills.zip` cannot be swapped.
- **Manifest:** `version: 3` (plaintext snapshots stay at `2`) and an `encryption` object:
  `{ scheme: "argon2id-xchacha20poly1305", version: 1, kdf: {memoryKib, iterations, parallelism}, salt, keyCheck }`.
  `keyCheck` is a sealed constant, so a wrong passphrase is reported before any artifact is
  downloaded. Artifact hashes and sizes describe the ciphertext. The remote path stays `v2/db-v6/...`.
- **Limits:** KDF parameters read from a manifest are capped (1 GiB, 16 passes, 8 lanes).
  Download size limits apply to the ciphertext, which is always at least as large as the plaintext.
- **Key cache:** the most recently derived key is kept in memory, keyed by a digest of the
  passphrase, so auto-sync does not run Argon2 on every upload.

Behaviour:

| Situation                                                          | Result                                                                                                  |
| ------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------- |
| Encryption on, upload                                              | `db.sql` and `skills.zip` are uploaded as ciphertext                                                    |
| Encryption on, but the passphrase cannot be read (keychain locked) | Upload refused (`sync.encryption.passphrase_unavailable`); nothing is sent in plaintext                 |
| Encrypted remote, no passphrase configured                         | `sync.encryption.passphrase_required`                                                                   |
| Encrypted remote, wrong passphrase, or modified data               | `sync.encryption.wrong_passphrase`                                                                      |
| Plaintext remote from an older version                             | Downloads as before, with or without a passphrase                                                       |
| Encryption off, encrypted remote, automatic upload                 | Refused (`sync.encryption.downgrade_refused`); a confirmed manual upload overwrites it                  |
| Older CC Switch reads an encrypted remote                          | Stops with "manifest protocol version is incompatible: v3", before downloading anything                 |
| Older CC Switch uploads                                            | Replaces the encrypted snapshot with plaintext. **Upgrade every device before you turn encryption on.** |

`fetch_remote_info` reports `encrypted` and `passphraseMatches` (`null` when the remote is not
encrypted or no passphrase is set).

Not encrypted: the manifest itself, which holds the device name, timestamps, hashes and the KDF
salt. Plain `http://` endpoints are still allowed; the existing `insecureTransport` warning is
unchanged. With encryption on, only the WebDAV/S3 credentials themselves are exposed on such links.

Losing the passphrase means the remote snapshot cannot be decrypted. Local data is not affected:
set a new passphrase and upload again.

## 3. Sync credentials in the OS keychain (CMD-04)

Backend: the `keyring` crate, using macOS Keychain, Windows Credential Manager, or the
freedesktop Secret Service (GNOME Keyring / KWallet) on Linux.
Service `com.ccswitch.desktop`; accounts `sync.webdav.password`, `sync.s3.secret-access-key`,
`sync.encryption.passphrase`.

- In memory, `AppSettings` keeps the real values, so the sync code is unchanged. When the file
  is written, each secret goes to the keychain and is blanked in `settings.json`, which records
  `passwordInKeychain` / `secretInKeychain` / `passphraseInKeychain: true`.
- **Migration:** at startup, any plaintext credential found in `settings.json` is moved to the
  keychain and the file is re-saved right away.
- **Fallback:** without a usable keychain (headless Linux without Secret Service, no D-Bus
  session), the secret stays in `settings.json` (`0600` on Unix) as before, and one warning is
  logged per secret.
- **Read failure** (for example a locked keychain): the entry is marked unresolved. Saves keep
  the keychain flag and never delete the entry, and sync fails with an authentication error
  until the keychain is available.
- Clearing a secret, or removing the WebDAV/S3 configuration, deletes the keychain entry.
- An unchanged value is never rewritten, so frequent settings saves do not touch the keychain.
- Tests never use the real keychain. Unit tests, and any process with `CC_SWITCH_TEST_HOME`
  set, use an in-memory backend. `CC_SWITCH_SECRET_BACKEND=keyring|memory|none` forces a
  backend; `none` also lets users opt out of the keychain. The real-keychain test is skipped
  when no backend exists.

Code: `src-tauri/src/secrets/{mod.rs,sync_credentials.rs}`.

## 4. Provider API-key vault (DB-02, PRODUCT-01)

Opt-in, in **Settings → Advanced → Data management → Store API keys in the system keychain**.
Off by default. The setting (`providerSecretVault` in `settings.json`) is device-local and is
never synced.

### What it does

Secret fields of every provider's `settings_config` (`*_API_KEY`, `*_AUTH_TOKEN`, `apiKey`,
`access_token`, `refresh_token`, `id_token`, `*_secret`, `password`, ...) and the `apiKey` of
universal providers are stored in the OS keychain (same backend as section 3). The database
keeps a string placeholder in their place:

```text
cc-switch-secret:v1:<app>/<provider id>/<JSON pointer>      (keychain account: provider/<ref>)
cc-switch-secret:v1:universal/<id>/apiKey
```

References are deterministic: the same provider and field map to the same keychain account on
every device. The JSON keeps its shape, so code that expects a string never breaks.

### Where it is wired in

| Path                                                                                         | Behaviour                                                                                                                                                                                                                                                                                                      |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| DAO reads (`get_all_providers`, `get_provider_by_id`, `get_current_omo_provider`, universal) | `provider_vault::for_read` replaces placeholders with the real values, through a per-process cache. Live-config writers, the proxy, failover, usage scripts and the UI therefore always see real keys. A placeholder whose entry is missing stays as it is (an obviously invalid key) and a warning is logged. |
| DAO writes (`save_provider`, `update_provider_settings_config`, universal save)              | With the vault on, `for_storage` writes the secrets to the keychain first and stores placeholders. If the keychain fails, the value is stored inline (as with the vault off) and a warning is logged; the settings panel reports it and **Retry** migrates it.                                                 |
| Overwrite / field removed                                                                    | Keychain entries this provider owns that the new row no longer references are deleted (`forget_stale`).                                                                                                                                                                                                        |
| Provider delete (incl. universal)                                                            | All entries owned by the provider (`<app>/<id>/...`) are deleted. Entries of other providers are never touched.                                                                                                                                                                                                |
| Duplicate, deep-link import, usage scripts                                                   | They read through the DAO (real values) and save through it, so a duplicate gets its own entries under the new id; placeholders are never copied verbatim between providers.                                                                                                                                   |

### Switching on and off

`set_provider_secret_vault(enabled)` (`secrets::provider_vault_admin`):

- **On:** probes the keychain (store, read back, delete). Without a working keychain — headless
  Linux without a Secret Service, no D-Bus session, `CC_SWITCH_SECRET_BACKEND=none` — it is
  **refused** with `secrets.vault.keychain_unavailable`, and the UI disables the switch with an
  explanation. An encrypted file next to the database would add nothing against local reads,
  so there is no file fallback. Otherwise: safety backup, flag on, then every row is migrated
  (keychain writes first, then the rows in one transaction). If the keychain fails part-way,
  the migrated rows are kept and the rest stays inline (reported, retry possible).
- **Off:** flag off, safety backup, every resolvable placeholder is inlined and the rows are
  persisted; only then are the keychain entries deleted. Placeholders whose entry cannot be
  read stay and are listed in the panel: those keys must be re-entered.
- Placeholders and inline values may coexist; every path handles the mixed state.
- Whole-settings saves (`save_settings`) cannot change the flag; only this command does.

`get_provider_secret_vault_status` reports the flag, the backend, keychain availability, and
the number of vaulted, still-inline and unresolved secrets (with the affected providers).

### SQL export, cloud sync and backups (decision)

**Exports and sync bundles never contain resolved secrets while the vault is on.**

- `export_sql_string` (Settings export) and `export_sql_string_for_sync` (WebDAV/S3) keep the
  placeholders. Any secret still stored inline (keychain failure) is blanked in the export, never
  written out. The live database is not changed by an export.
- The SQL file starts with the usual header followed by an explicit warning:
  `-- cc-switch-vault: N provider secret(s) live in the OS keychain of the exporting device ...`
  and, if any were blanked, how many were left out.
- **Keys are not carried to other devices.** On a device that does not have the keychain
  entries, the user re-enters the keys. Section 2 encryption still protects everything else in
  the bundle. A future `secrets.bin` artifact sealed with the sync key could carry them; it is
  not implemented.

Import, sync download and backup restore (`Database::adapt_imported_secrets`, run on the
staging copy before it replaces the live database):

- A placeholder whose entry is missing here takes the key **this device already holds** for the
  same provider and field (inline, or from its own keychain). A sync download from a vault-on
  device therefore never wipes the keys of a vault-off device.
- With the vault on here, inline keys in the imported data move to this keychain.
- With the vault off here, resolvable placeholders become inline values again.
- What remains unresolved is logged and shown in the settings panel (re-enter those keys).

Binary backups in `backups/` are copies of the database, so they hold placeholders. Restoring one
on the same device works while the keychain entries exist; turning the vault off, or deleting a
provider, removes entries, after which older backups show those keys as unresolved.

### Not covered

- The CLI config files that CC Switch writes for Claude Code, Codex, Gemini and others still
  contain the active key by design. The vault keeps keys out of `cc-switch.db`, its backups,
  SQL exports and sync bundles.
- Secrets inside TOML strings (Codex `config` text, e.g. `experimental_bearer_token`), the
  provider `meta` column (usage-script credentials), common-config snippets and
  `global_proxy_url` (`user:pass@`, device-local and never synced) are not vaulted yet.
- There is no write guard in the live-file writers: a placeholder can reach a live file only
  when its entry is missing, and it is then an invalid key, not a crash.
- Malware running as the same user can read the keychain entries (see the threat model).

### Tests

All tests use the in-memory backend (unit tests always; any process with
`CC_SWITCH_TEST_HOME`, or `CC_SWITCH_SECRET_BACKEND=memory`). They cover: the
placeholder/resolve/restore primitives, DAO round trips (reads return real values, rows hold
placeholders), toggle on/off over a seeded database, deletion removing entries, universal
providers, exports with the vault on (no secrets, warning present, inline secrets blanked), and a
sync download onto a device that keeps its own key.

Code: `src-tauri/src/secrets/{provider_vault.rs,provider_vault_admin.rs,commands.rs}`,
`database/dao/{providers.rs,universal_providers.rs}`, `database/backup.rs`,
`src/components/settings/SecretsVaultSettings.tsx`.
