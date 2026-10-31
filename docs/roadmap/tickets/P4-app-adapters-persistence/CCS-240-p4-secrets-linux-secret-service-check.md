# CCS-240: [secrets] Exercise KeyringSecretStore against a headless gnome-keyring and the no-D-Bus fallback

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P4`, `p3-low`, `area:secrets`

> **Ticket key:** `p4-secrets-linux-secret-service-check` · **Phase:** P4 · **Priority:** p3-low · **Effort:** M
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-172, CCS-005). Source tickets: [CCS-172](../P4-app-adapters-persistence/CCS-172-p4-keychain-secrets.md), [CCS-005](../P4-app-adapters-persistence/CCS-005-epic-p4.md).

## Problem
Every test uses the file store because `keychain_allowed()` returns false under `cfg(test)` (`src-tauri/src/secret_store.rs:452`), so `KeyringSecretStore` (66) and `probe()` (100) never run. CCS-172's "manual check on macOS, Windows and Linux" (SECRET_STORAGE.md checklist steps 1-5) can be covered for the Linux Secret Service API here: dbus-run-session/dbus-daemon/xvfb-run exist and apt can install gnome-keyring 46.1 and libsecret-tools.

## Proposed change
- Add `#[ignore]` tests in a `secret_store::keyring_live` module (the module is private, `lib.rs:41`), using `KeyringSecretStore::new("cc-switch-test-<pid>")` directly:
  - `probe()` is Ok; set/get/delete round trip; get after delete is None.
  - A `SecretPolicy` over the real store: hydrate/externalize `WEBDAV_PASSWORD_KEY` and `copilot_token_key("acct")`; `forget()` removes the entry; plaintext mode (`SecretPolicy::new(store, true)`) + `release()` writes the value back inline and deletes the entry; the cache still reports an out-of-band delete.
  - `probe_fails_without_dbus`: with `DBUS_SESSION_BUS_ADDRESS` unset, probe is Err and the `policy_for_dir()` fallback (~484-500) yields a FileSecretStore whose secrets.json is 0600 after externalize.
- Add `scripts/check-linux-secret-service.sh`: `dbus-run-session -- sh -c 'echo -n pw | gnome-keyring-daemon --unlock --components=secrets >/dev/null; cargo test --manifest-path src-tauri/Cargo.toml --lib secret_store::keyring_live -- --ignored --test-threads=1'`, then the no-D-Bus run via `env -u DBUS_SESSION_BUS_ADDRESS`, then `secret-tool search --all service cc-switch-test-*` to confirm cleanup.
- Optional, stronger: run the built debug binary under `dbus-run-session` + `xvfb-run` with a seeded HOME (plaintext webdav password and copilot token), check fields are blanked and `secret-tool lookup` returns the value; repeat with `CC_SWITCH_SECRET_STORE=plaintext` for the downgrade.
- Record the result in CCS-172 and in the docs/dev/SECRET_STORAGE.md checklist as the Linux/GNOME Keyring check.

## Acceptance criteria / verification
- [x] `scripts/check-linux-secret-service.sh` exits 0 and all ignored tests pass.
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib secret_store` (non-ignored) and clippy -D warnings still pass.

## Dependencies
None.

## Implementation notes

**Status: Done.** Commit c655b24 (code and script); this ticket file, CCS-172 and
`docs/dev/SECRET_STORAGE.md` are updated in the follow-up docs commit.

- `src-tauri/src/secret_store.rs`
  - Backend selection moved from `policy_for_dir()` into `select_policy(dir, mode, keychain)`;
    the probe-and-log part of `keychain()` is now `probe_keychain(service)`. Behaviour is the
    same; the split lets tests drive the real fallback without the `cfg(test)` guard in
    `keychain_allowed()`.
  - New non-ignored test `tests::select_policy_falls_back_to_file_when_keychain_is_unavailable`
    (Auto/Plaintext fall back to the file store, a present keychain is chosen, `file` mode never
    probes).
  - New `keyring_live` module (`cfg(all(test, target_os = "linux"))`, all `#[ignore]`), using
    `KeyringSecretStore::new` with service `CC_SWITCH_KEYRING_TEST_SERVICE` or
    `cc-switch-test-<pid>`:
    `probe_and_round_trip`, `policy_externalizes_hydrates_and_forgets` (`WEBDAV_PASSWORD_KEY`,
    `copilot_token_key("acct")`, `forget()`), `plaintext_mode_writes_back_and_releases`,
    `cache_masks_an_out_of_band_delete` (a cached instance keeps returning the old value after
    another instance deletes the entry; a fresh instance sees `None`; deleting again succeeds),
    and `probe_fails_without_dbus` (probe is Err, `select_policy` with the real probe yields the
    file store, `secrets.json` is 0600 after externalize).
- `scripts/check-linux-secret-service.sh`: builds the test binary, then under `dbus-run-session`
  unlocks a gnome-keyring whose files live in a temp `XDG_DATA_HOME`, runs the ignored tests
  (`--test-threads=1`, skipping the no-D-Bus test), and fails if
  `secret-tool search --all service cc-switch-test-<pid>` finds entries. It then runs
  `probe_fails_without_dbus` with `DBUS_SESSION_BUS_ADDRESS` unset and `XDG_RUNTIME_DIR` set to an
  empty directory. zbus falls back to `$XDG_RUNTIME_DIR/bus` (or `/run/user/<uid>/bus`), so unsetting
  the address alone is not enough on a desktop. `secret-tool search` does not take globs, so the
  script picks the service name and passes it to the tests.
- `docs/dev/SECRET_STORAGE.md`: added a "Linux Secret Service (automated)" section to the
  checklist. CCS-172: recorded the Linux/GNOME Keyring result.

Not done: the optional run of the built app binary under `dbus-run-session` + `xvfb-run` with a
seeded HOME. The API-level tests cover the same store and policy paths. KWallet and interactive
unlock prompts are still not covered (see CCS-172).

### Review

Independent review re-ran `scripts/check-linux-secret-service.sh` (exit 0) and checked both
acceptance criteria, the `select_policy` refactor (same behaviour; `policy_for_dir` keeps the
`keychain_allowed()` guard) and that keyring 4.2 on Linux uses the zbus Secret Service store
with `service`/`username` attributes. Gap fixed: the cleanup check alone would also pass if
no entry ever reached gnome-keyring, and every test talked only to in-process instances. Added
`shares_entries_with_secret_tool`: the script seeds `test.external` with `secret-tool store`
(`CC_SWITCH_KEYRING_TEST_SEED`), the test reads it through `KeyringSecretStore` and
overwrites it, and the script checks the new value with `secret-tool lookup` before clearing
it. Re-verified: script exit 0 (Secret Service run 5 passed, no entries left; no D-Bus run 1
passed), `cargo test --lib` 2861 passed / 0 failed / 11 ignored, clippy `-D warnings` (with
and without `--tests`) clean.

### Verification

- `apt-get install gnome-keyring libsecret-tools` (gnome-keyring 46.1-2ubuntu0.2, Ubuntu 24.04).
- `scripts/check-linux-secret-service.sh`: exit 0. Secret Service run: 4 passed; "no entries left
  under service cc-switch-test-23953". No D-Bus run: `probe_fails_without_dbus` 1 passed.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib secret_store`: 20 passed, 0 failed,
  5 ignored.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 2861 passed, 0 failed, 10 ignored.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` and the same with `--tests`:
  clean. `cargo fmt`: no changes. `bash -n` on the script: OK. shellcheck is not installed.
