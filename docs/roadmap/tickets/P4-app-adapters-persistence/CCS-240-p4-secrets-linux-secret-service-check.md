# CCS-240: [secrets] Exercise KeyringSecretStore against a headless gnome-keyring and the no-D-Bus fallback

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P4`, `p3-low`, `area:secrets`

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
- [ ] `scripts/check-linux-secret-service.sh` exits 0 and all ignored tests pass.
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib secret_store` (non-ignored) and clippy -D warnings still pass.

## Dependencies
None.
