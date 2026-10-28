# CCS-084: [settings] Save settings.json atomically and back up a corrupt file instead of silently resetting

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p1-high`, `area:settings`

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
- [ ] Tests: invalid JSON, then load and save, leaves a .corrupt backup with the original bytes (or the save is refused)
- [ ] The file mode is 0600 both when new and when it already existed; no tmp files are left
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml settings::`

## Dependencies
p1-atomic-write-perms
