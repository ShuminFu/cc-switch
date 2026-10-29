# CCS-085: [settings] Hold the settings write lock across merge, save and swap

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p2-medium`, `area:settings`

> **Ticket key:** `p1-settings-save-race` · **Phase:** P1 · **Priority:** p2-medium · **Effort:** S · **Depends on:** `p1-settings-atomic-save`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding backend-save-settings-race. save_settings snapshots the settings outside any lock and then replaces the whole struct, so concurrent mutate_settings commits (migration markers, sync status, current provider) are lost. update_settings also writes the file before taking the lock, so the file and memory can disagree.

## Evidence
- src-tauri/src/commands/settings.rs:13-52 (merge), 66-71; settings.rs:748-774; concurrent writers codex_history_migration.rs:127/151/182/256, settings.rs:982, 1112/1119

## Proposed change
- Add `update_settings_with(|existing| ...)`, which takes the write lock, merges against the guard, normalizes, saves, swaps, and returns the previous value for the unify_codex diff.
- Make update_settings lock before writing the file.

## Acceptance criteria / verification
- [x] Test: snapshot, then mark_codex_provider_template_migrated, then the save path; the marker is still set (fails before the fix)
- [x] Stress test: N set_webdav_sync_settings calls racing saves leave the file equal to get_settings()
- [x] `cargo test --manifest-path src-tauri/Cargo.toml settings`

## Dependencies
p1-settings-atomic-save

## Implementation notes

**Summary.** Added `settings::update_settings_with(|existing| ...)`, which takes the settings write lock, builds the next value from the guarded (latest) settings, normalizes paths, writes `settings.json` and swaps the in-memory cache, returning the previous value. `update_settings` now delegates to it, so the file write happens under the lock as well. The `save_settings` command goes through a new helper, `persist_settings_for_save`, which runs `merge_settings_for_save` inside that closure and returns the previous settings, which the `unify_codex_session_history` diff needs. The rollback after a failed Codex live rewrite also merges against the latest value under the lock, so it no longer wipes markers committed in the meantime. A test-only `SharedSettingsStore` handle lets threads share one scoped settings store, which the stress test needs.

**Files.**
- `src-tauri/src/settings.rs`: `update_settings_with`, `update_settings` delegating to it, and the `#[cfg(test)] SharedSettingsStore`
- `src-tauri/src/commands/settings.rs`: `persist_settings_for_save`, `save_settings` and the rollback using it, and the tests in `tests::save_race`

**Commits.** `12dfe8b` (code and tests), plus the follow-up commit for this ticket file.

**Verification.**
- `save_race::save_after_marker_commit_keeps_marker`: takes a frontend snapshot, then calls `mark_codex_provider_template_migrated`, then the save path. The marker is still set and the file matches `get_settings()`. Before the fix this test also passes, because there is no hook to land a commit inside the old save's unlocked snapshot-to-swap window. The stress test below is the one that fails before the fix.
- `save_race::concurrent_sync_settings_and_saves_keep_file_and_memory_in_sync`: 200 `set_webdav_sync_settings` and `mark_codex_provider_template_migrated` calls race against 2 saver threads. The last WebDAV settings and marker survive, and the file equals `get_settings()`. With the old save path and old `update_settings` restored temporarily, it failed in 3 of 3 runs (stale WebDAV URL, e.g. `/163` instead of `/199`). With the fix it passes.
- `cargo fmt --check`: clean. `cargo clippy -- -D warnings`: clean.
- `cargo test --lib`: 2235 passed, 0 failed, 3 ignored.
- `cargo test settings`: all passed (41 in lib plus the integration-test matches), 0 failed.
