# CCS-085: [settings] Hold the settings write lock across merge, save and swap

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p2-medium`, `area:settings`

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
- [ ] Test: snapshot, then mark_codex_provider_template_migrated, then the save path; the marker is still set (fails before the fix)
- [ ] Stress test: N set_webdav_sync_settings calls racing saves leave the file equal to get_settings()
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml settings`

## Dependencies
p1-settings-atomic-save
