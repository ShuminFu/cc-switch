# Secrets at rest and in sync

This page covers how CC Switch protects credentials on disk and in cloud sync, what is
already in place, and the remaining work for the provider API-key vault
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

## 4. Provider API-key vault (DB-02, PRODUCT-01): design and remaining steps

### Status

`src-tauri/src/secrets/provider_vault.rs` implements and tests the building blocks:

- Secret field detection (`*_API_KEY`, `*_AUTH_TOKEN`, `apiKey`, `access_token`,
  `refresh_token`, `id_token`, `*_secret`, `password`, ...).
- `extract`: moves the secrets of one `settings_config` into the keychain and replaces each one
  with a string placeholder, `cc-switch-secret:v1:<app>/<provider id>/<JSON pointer>`. The JSON
  keeps its shape, so readers never meet an unexpected type. All keychain writes happen first:
  on failure the JSON is untouched and any entries already written are removed again.
- `resolve`: returns a copy with the placeholders replaced, or fails as a whole if an entry is missing.
- `restore`: the opposite migration. Placeholders are replaced and entries are deleted only
  after every placeholder resolved.
- `forget`: deletes a provider's entries.
- `migrate_rows_to_vault` / `migrate_rows_from_vault`: bulk migration in either direction.
  Each stops at the first failure and returns how many rows were done, so callers persist
  exactly those rows.

The vault is **not wired in** and there is **no user-facing setting yet**. A switch that does
nothing would mislead users, so the setting is added together with the integration below.
Once added, it must default to OFF.

### Remaining steps

1. **Setting:** `providerSecretVault: bool` in `AppSettings`, default `false`. Device-local:
   the setting is not synced.
2. **DAO boundary** (`database/dao/providers.rs`). Read paths call `resolve` on
   `settings_config` (cache resolved values per process). Write paths call `extract` when the
   flag is on. Every other module keeps seeing plaintext. `universal_providers` and the
   `settings` table (common-config snippets, `global_proxy_url` with `user:pass@`) need the
   same treatment.
3. **Toggle migration** (in a transaction, after a safety backup):
   - On: `migrate_rows_to_vault`, then persist the migrated rows.
   - Off: `migrate_rows_from_vault`, then persist.
   - On partial failure, persist only the completed rows and report the rest. A mixed state
     is valid, because placeholders and inline values can coexist.
4. **Live files:** `services/provider/live.rs` and the per-app writers already receive
   resolved configs through the DAO. Add a guard that refuses to write any string starting
   with `cc-switch-secret:v1:` into a live file.
5. **SQL export, sync and backups:** placeholders make these artifacts secret-free, but useless
   on another machine. Options:
   - (a) Resolve placeholders when exporting, and require sync encryption (section 2) when
     the vault is on.
   - (b) Add a `secrets.bin` artifact sealed with the sync key.

   Option (a) is simpler and recommended. A backup restored on the same device works as long
   as the keychain entries exist; `restore_from_backup` must check with `resolve` and warn
   about missing entries.

6. **Provider delete/duplicate:** call `forget` on delete. On duplicate, resolve and then
   extract under the new id.
7. **Deep-link import / usage scripts / proxy:** these already read through the DAO. Check
   that none of them store placeholders back verbatim.
8. **No-keychain fallback:** PRODUCT-01 asks for an encrypted file store when no Secret
   Service exists. A file encrypted with a key stored next to it adds nothing against local
   reads, so the recommended behaviour is to refuse enabling the vault when
   `secrets::backend()` is unavailable, and to say so in the UI.
9. **Tests:** DAO round-trip with the memory backend; toggle on/off migration over a seeded
   DB; export/sync with the vault on; a guard test that no placeholder reaches a live file.

Even when complete, the vault does not remove plaintext keys from disk: the CLI config files
that CC Switch writes for Claude Code, Codex, Gemini and others contain them by design. The
vault mainly keeps keys out of `cc-switch.db`, its backups and SQL exports.
