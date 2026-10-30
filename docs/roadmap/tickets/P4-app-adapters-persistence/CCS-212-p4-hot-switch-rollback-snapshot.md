# CCS-212: [takeover] Keep the raw byte snapshot when a failed hot switch rolls back the live backup

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P4`, `p3-low`, `area:proxy`

> **Ticket key:** `p4-hot-switch-rollback-snapshot` · **Phase:** P4 · **Priority:** p3-low · **Effort:** S · **Depends on:** `p4-takeover-crash-e2e`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the review of CCS-152.

## Problem
CCS-152 added `proxy_live_backup.raw_snapshot`, the original bytes of the app's live files, so that a restore after takeover gives back byte-identical files. `rollback_hot_switch_preparation` (`src-tauri/src/services/proxy/hot_switch.rs:120-143`) restores the previous backup with `save_live_backup(app, &backup.original_config)` after a failed hot switch. That writes the JSON backup only and drops the raw snapshot. After any failed hot switch, the next restore falls back to the JSON backup. The settings are still correct, but hand-written formatting is lost: re-indented settings.json/auth.json, and a missing trailing newline in .env.

## Proposed change
Carry the raw snapshot through the rollback. Read the full previous backup row, snapshot included, before the hot switch prepares a new backup, and write the same row back on rollback. A DAO helper can save backup and snapshot together. Do not record a new snapshot during rollback, because the live files may already have been changed.

## Acceptance criteria / verification
- [x] A test takes over an app with hand-written live files, forces a hot switch failure after the backup was rewritten, then restores, and asserts the live files are byte-identical to the original. It fails before the fix.
- [x] Under `takeoverEngine = legacy`, behaviour is unchanged (no snapshot).
- [x] `cargo test --test takeover_crash`, the full `cargo test --no-fail-fast` and `cargo clippy --all-targets -- -D warnings` pass.

## Dependencies
CCS-152.

## Implementation notes
- **Change:** `hot_switch_provider_inner` now reads the previous backup's raw snapshot (`get_live_backup_raw_snapshot`) together with the previous backup row, only under `takeoverEngine = unified`. `rollback_hot_switch_preparation` takes the row and snapshot as one argument and writes them back through the new DAO helper `save_live_backup_with_raw_snapshot`, which restores `original_config`, `backed_up_at` and `raw_snapshot` in one `INSERT OR REPLACE`. Rollback never captures a new snapshot. Under `legacy` the snapshot is not read, so the restored row has none, as before.
- **Files:** `src-tauri/src/database/dao/proxy.rs`, `src-tauri/src/services/proxy/hot_switch.rs`, `src-tauri/tests/takeover_crash.rs`.
- **Tests:** `takeover_crash::failed_hot_switch_keeps_the_takeover_snapshot` covers claude, codex, gemini and grokbuild. The additive apps never rewrite the backup on a hot switch. For each app the test takes over hand-written live files, then installs a SQLite trigger so the provider commit fails after the backup was rewritten, and the hot switch is rolled back. The test then checks that the snapshot is back on the row, crash-recovers, and checks that HOME is byte-identical to the original. `legacy_engine_failed_hot_switch_records_no_snapshot` checks that the legacy engine leaves no snapshot. With the rollback reverted to `save_live_backup`, the first test fails with "claude: rollback must write the takeover snapshot back".
- **Commits:** ee3196b (code + tests); this note is in a follow-up commit.
- **Verification:** `cargo fmt` clean; `cargo clippy --all-targets -- -D warnings` clean; `cargo test --test takeover_crash`: 10 passed; `cargo test --no-fail-fast` (all targets): 3047 passed, 0 failed.
