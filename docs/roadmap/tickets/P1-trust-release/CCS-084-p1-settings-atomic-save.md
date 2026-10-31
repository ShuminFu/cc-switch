# CCS-084: [settings] Save settings.json atomically and back up a corrupt file instead of silently resetting

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p1-high`, `area:settings`

> **Ticket key:** `p1-settings-atomic-save` · **Phase:** P1 · **Priority:** p1-high · **Effort:** S · **Depends on:** `p1-atomic-write-perms`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding backend-settings-nonatomic-save.
- save_settings_file truncates and writes in place with no fsync.
- On a parse failure, load_from_file returns defaults, and the next save overwrites the file. That loses WebDAV/S3 credentials, current providers and the local_migrations markers, so migrations re-run.

## Evidence
- src-tauri/src/settings.rs:683-691 (truncate+write), 696 non-unix fs::write, 639-661 load_from_file, 705, 750, 771

## Proposed change
- Use atomic_write with 0600 (from p1-atomic-write-perms).
- On a parse error, copy the file to settings.json.corrupt-<ts>.
- Set a `load_failed` flag that blocks overwriting until the user confirms, and surface it to the UI.

## Acceptance criteria / verification
- [x] Tests: invalid JSON, then load and save, leaves a .corrupt backup with the original bytes (or the save is refused)
- [x] The file mode is 0600 both when new and when it already existed; no tmp files are left
- [x] `cargo test --manifest-path src-tauri/Cargo.toml settings::`

## Dependencies
p1-atomic-write-perms

## Implementation notes

**Summary**
- `save_settings_file` now writes through `config::atomic_write_secret` (tmp file + `sync_all` + rename + dir fsync; unix mode forced to 0600 on create *and* rewrite, owner-only DACL on Windows). The in-place truncate+write and the non-unix `fs::write` branch are gone; `src/settings.rs` was dropped from the `OpenOptions` allowlist in `tests/write_lint.rs`.
- `AppSettings::load_from_file` reads raw bytes (`serde_json::from_slice`, so invalid UTF-8 is treated as corrupt too) and returns `(settings, Option<SettingsLoadFailure>)`:
  - missing file → defaults, no failure;
  - parse error → original bytes copied to `settings.json.corrupt-<YYYYmmdd-HHMMSS>[-n]` (0600, via `atomic_write_secret`), defaults used, failure recorded with `backup_path`; later saves are allowed because the original bytes are preserved;
  - read error other than NotFound, or backup failure → failure recorded with `backup_path: None`; `update_settings` / `mutate_settings` are **refused** until the user acknowledges.
- The settings cache is now a `SettingsStore { settings, load_failure }` (derefs to the old `RwLock<AppSettings>`); `reload_settings` refreshes the failure state.
- New public API + Tauri commands (registered in `lib.rs`): `get_settings_load_failure` (returns `{ path, error, backupPath }` or null) and `acknowledge_settings_load_failure` (clears the flag, unblocking saves).

**Files**: `src-tauri/src/settings.rs`, `src-tauri/src/commands/settings.rs`, `src-tauri/src/lib.rs`, `src-tauri/tests/write_lint.rs`.

**Commits**: ac5e1db (code + tests), plus this ticket-notes commit.

**Tests added** (`settings::tests::persistence`): `invalid_json_is_backed_up_before_save_overwrites_it`, `invalid_utf8_is_treated_as_corrupt_and_backed_up`, `save_is_refused_when_unreadable_file_cannot_be_backed_up`, `missing_or_valid_file_records_no_failure`, `save_is_atomic_owner_only_and_leaves_no_tmp_files` (0600 on create and after chmod 0644 + rewrite; directory contains only `settings.json`). The invalid-JSON test fails on the old code (no backup is made, and the save overwrites the only copy).

**Verification**
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check` → clean
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` → clean
- `cargo test --manifest-path src-tauri/Cargo.toml --lib settings::` → 18 passed, 0 failed
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast` → 2397 passed, 0 failed (lib: 2233 passed, 3 ignored; `write_lint` 3/3 after removing the stale allowlist entry)

**Remaining (why Partial)**
- Showing the failure in the UI is frontend-lane work and is not done here: on startup the frontend should call `get_settings_load_failure`. If `backupPath` is set, it should show a notice with that path. If it is null, it should explain that saving is blocked and offer a confirm action that calls `acknowledge_settings_load_failure`. The backend commands are in place; there is no TS wrapper or i18n strings yet.
- The Windows owner-only ACL path relies on `atomic_write_secret` (CCS-083) and was not exercised on a Windows runner here.

### Remaining after triage

- Blocked: Windows owner-only DACL path of atomic_write_secret. Needs a Windows machine or Windows CI run.
- Follow-up [CCS-225](../P1-trust-release/CCS-225-p1-settings-load-failure-notice.md): Show a startup notice and confirm flow when settings.json failed to load.
- Done in CCS-225: the frontend now shows the load failure on startup (`SettingsLoadFailureNotice` in `src/components/app-shell/AppConfirmDialogs.tsx`): a persistent warning with the backup path, or a confirm dialog that calls `acknowledge_settings_load_failure` when saving is blocked. Marked Done; the only open item is the Windows owner-only DACL check above, which still needs a Windows runner.
