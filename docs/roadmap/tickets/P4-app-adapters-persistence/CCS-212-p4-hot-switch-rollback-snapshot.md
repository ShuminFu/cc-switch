# CCS-212: [takeover] Keep the raw byte snapshot when a failed hot switch rolls back the live backup

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P4`, `p3-low`, `area:proxy`

> **Ticket key:** `p4-hot-switch-rollback-snapshot` · **Phase:** P4 · **Priority:** p3-low · **Effort:** S · **Depends on:** `p4-takeover-crash-e2e`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the review of CCS-152.

## Problem
CCS-152 added `proxy_live_backup.raw_snapshot`, the original bytes of the app's live files, so that a restore after takeover gives back byte-identical files. `rollback_hot_switch_preparation` (`src-tauri/src/services/proxy/hot_switch.rs:120-143`) restores the previous backup with `save_live_backup(app, &backup.original_config)` after a failed hot switch. That writes the JSON backup only and drops the raw snapshot. After any failed hot switch, the next restore falls back to the JSON backup. The settings are still correct, but hand-written formatting is lost: re-indented settings.json/auth.json, and a missing trailing newline in .env.

## Proposed change
Carry the raw snapshot through the rollback. Read the full previous backup row, snapshot included, before the hot switch prepares a new backup, and write the same row back on rollback. A DAO helper can save backup and snapshot together. Do not record a new snapshot during rollback, because the live files may already have been changed.

## Acceptance criteria / verification
- [ ] A test takes over an app with hand-written live files, forces a hot switch failure after the backup was rewritten, then restores, and asserts the live files are byte-identical to the original. It fails before the fix.
- [ ] Under `takeoverEngine = legacy`, behaviour is unchanged (no snapshot).
- [ ] `cargo test --test takeover_crash`, the full `cargo test --no-fail-fast` and `cargo clippy --all-targets -- -D warnings` pass.

## Dependencies
CCS-152.
