# CCS-172: [security] Store secrets in the OS keychain or credential manager

**Status:** Partial · **Kind:** feature · **Labels:** `feature`, `phase:P4`, `p2-medium`, `area:security`

> **Ticket key:** `p4-keychain-secrets` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** L · **Depends on:** `p1-oauth-token-store-hardening`, `p4-per-app-settings-state`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Backend idea 6 plus the critique on the OAuth token stores. WebDAV/S3 passwords, provider API keys and OAuth tokens sit in plaintext in settings.json, SQLite and token JSON files.

## Proposed change
- Use the keyring crate behind a SecretStore trait, with a file fallback (0600) on headless Linux.
- Migrate the WebDAV/S3 secrets and the Copilot/Codex tokens first.
- Keep provider API keys in the live config files, since the CLIs need them.

## Acceptance criteria / verification
- [x] Tests with a mock SecretStore: migration moves secrets and blanks the settings fields
- [x] A downgrade path is documented
- [ ] Manual check on macOS, Windows and Linux
- [x] `cargo test --manifest-path src-tauri/Cargo.toml secret_store`

## Dependencies
p1-oauth-token-store-hardening, p4-per-app-settings-state

## Implementation notes

**Status: Partial.** The code, the tests and the downgrade documentation are done. The manual
check on real macOS, Windows and Linux desktops cannot run in this headless Linux CI container.

### Summary
- New `src-tauri/src/secret_store.rs`:
  - `SecretStore` trait (`get`/`set`/`delete`) with three implementations:
    - `KeyringSecretStore`: the `keyring` 4.x crate, which uses macOS Keychain, Windows
      Credential Manager or the zbus Secret Service. It shares the `zbus` 5 already in the tree
      and keeps a small cache so repeated identical writes skip the keychain.
    - `FileSecretStore`: `<config dir>/secrets.json`, written via `atomic_write_secret` (0600 /
      owner-only ACL) under one process-wide lock.
    - `MemorySecretStore`: a test-only mock that can be told to fail writes.
  - `SecretPolicy` handles hydrate on load, externalize on save, release after a plaintext
    write-back, and forget for removed accounts. `policy_for_dir()` probes the keychain once.
    If the probe fails (headless Linux or no Secret Service), it falls back to the file store.
  - Test builds and `CC_SWITCH_TEST_HOME` always use the file store and never the real keychain.
  - `CC_SWITCH_SECRET_STORE` accepts `auto|keychain` (default), `file` or `plaintext`
    (downgrade mode).
- `settings.rs`:
  - The WebDAV password and the S3 Secret Access Key are hydrated from the store on load.
    On every save they are written to the store and blanked in `settings.json`.
  - If a store write fails, the value stays in the 0600 file, so nothing is lost.
  - Removing a sync config or clearing its secret deletes the store entry.
  - The new `settings::migrate_secrets()` runs at startup (`lib.rs`, step 1.4) and rewrites the
    file only when plaintext is still present. The in-memory settings keep the real values, so
    sync code and the "preserve empty password" logic are unchanged.
- Copilot (`copilot_auth.json` → `copilot.github_token.<id>`) and Codex OAuth
  (`codex_oauth_auth.json` → `codex_oauth.refresh_token.<id>`):
  - Tokens are hydrated on load. Plaintext tokens migrate right away, with the file rewritten
    with `""`.
  - On save, tokens are written to the store. Entries for removed accounts and for `clear_auth`
    are deleted.
  - The token fields got `#[serde(default)]`. Proxy edits are limited to the two auth files.
- Provider API keys stay in the live configs, as the ticket requires.
- Downgrade path: `docs/dev/SECRET_STORAGE.md` (start the current version once with
  `CC_SWITCH_SECRET_STORE=plaintext`, then install the old one). The same file has the manual
  cross-platform checklist.

### Files
`src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `src-tauri/src/secret_store.rs` (new),
`src-tauri/src/settings.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/commands/settings.rs` (test
expectation), `src-tauri/src/proxy/providers/copilot_auth.rs`,
`src-tauri/src/proxy/providers/codex_oauth_auth.rs`, `src-tauri/tests/config_writers.rs`, and the
`tests/fixtures/live/{copilot_auth,codex_oauth}/*.snap` snapshots (tokens are now `""` in the store
file and present in `secrets.json`), plus `docs/dev/SECRET_STORAGE.md` (new).

### Commits
- `22e3b3e` feat(security): store sync passwords and OAuth tokens in the OS keychain (CCS-172)

### Verification
- `cargo test --manifest-path src-tauri/Cargo.toml secret_store`: 16 passed, 0 failed. The tests
  cover:
  - migration moves the secrets into the mock store and blanks the settings fields
  - a status-only save also migrates
  - removing a config deletes its secret
  - a failed store write keeps the plaintext
  - plaintext/downgrade mode writes the secrets back
  - token migration on load for Copilot and Codex
  - token deletion on remove and on clear
  - file store round trip with mode 0600
  - test builds never select the keychain
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 2482 passed, 0 failed, 3 ignored.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast` (lib plus all integration
  tests): 2683 passed, 0 failed.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`: clean.
  `cargo fmt --check`: clean.

### Remaining
- Manual check on macOS, Windows and Linux desktops, using the checklist in
  `docs/dev/SECRET_STORAGE.md`. This covers real keychain prompts, the Windows Credential Manager
  entries, the GNOME Keyring/KWallet path and the headless fallback. It needs those platforms or a
  human tester.
- Not in scope and unchanged: the legacy `webdavBackup` value in settings.json, and the v1
  single-account Copilot `github_token`. The v1 token is externalized on its first save after the
  existing v1→v3 migration.
