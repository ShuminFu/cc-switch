# CCS-169: [settings] Add a retention and DB-size panel

**Status:** Done · **Kind:** feature · **Labels:** `feature`, `phase:P4`, `p3-low`, `area:settings-ui`

> **Ticket key:** `p4-retention-panel` · **Phase:** P4 · **Priority:** p3-low · **Effort:** S · **Depends on:** `p4-rollup-retention`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Detail retention is hard-coded to 30 days, and rollups are never pruned.

## Proposed change
A settings panel showing the DB size, row counts per table and the RetentionPolicy controls, with a "run maintenance now" button.

## Acceptance criteria / verification
- [x] RTL test; a command test that maintenance respects the new policy
- [x] `pnpm test:unit`

## Dependencies
p4-rollup-retention

## Implementation notes

**Summary**
- Backend: `Maintenance::storage_stats(db)` returns `DbStorageStats { sizeBytes, freeBytes,
  tables: [{ name, rows }] }` (main DB `page_count × page_size`, free pages × page size, a
  `COUNT(*)` per user table from `sqlite_master`, sorted by name). `MaintenanceReport` is now
  serialisable. Two new commands in `commands/import_export.rs`: `get_db_storage_stats` and
  `run_db_maintenance`; the latter calls `Maintenance::run` with
  `effective_retention_policy()` re-read on every call, so a policy saved in settings takes
  effect on the next "run now" without a restart. Both DTOs are exported to
  `src/shared/bindings.ts` via tauri-specta.
- Frontend: `RetentionPanel` (Settings > Advanced > Data Retention, after Backup & Restore)
  shows the DB size / reclaimable space and a row-count table, `Select`s for
  `usageDetailRetentionDays` (default 30) and `usageRollupRetentionDays` (0 = keep forever;
  hint notes the backend never lets it be shorter than the detail period), auto-saved through
  the existing `handleAutoSave`, and a "Run maintenance now" button that toasts the
  `MaintenanceReport` counts and refreshes the stats, backup list and usage queries. A
  hand-edited non-preset day value is still shown. The backup count stays in Backup & Restore
  (the panel's hint points there) to avoid two controls for one setting.
- `maintenanceApi` in `src/lib/api/settings.ts`, the two settings fields in `Settings`
  (`src/types.ts`), i18n for en / zh / zh-TW / ja; the two commands are listed in
  `tests/msw/unmockedCommands.ts` with typed `ResultOverrides` in `tests/msw/commands.ts`.

**Files**
- `src-tauri/src/database/{maintenance.rs, mod.rs}`, `src-tauri/src/commands/import_export.rs`,
  `src-tauri/src/lib.rs`, `src-tauri/src/bindings.rs`, `src/shared/bindings.ts` (regenerated)
- `src/components/settings/{RetentionPanel.tsx (new), SettingsPage.tsx, BackupListSection.tsx}`
  (`formatBytes` exported for reuse), `src/lib/api/{settings.ts, index.ts}`, `src/types.ts`,
  `src/i18n/locales/{en,zh,zh-TW,ja}.json`
- `tests/components/RetentionPanel.test.tsx` (new), `tests/msw/{commands.ts, unmockedCommands.ts}`

**Commits**
- `d7bf2f4` feat(settings): add a data retention and DB-size panel
- follow-up commit: these notes

**Tests**
- Command test `commands::import_export::tests::run_maintenance_uses_the_retention_policy_saved_in_settings`:
  a 40-day-old detail row survives when settings say 60 days, and is rolled up after the
  setting is saved as 20 days (same DB, same command path).
- `database::maintenance::tests::storage_stats_reports_size_and_row_counts`
- RTL `tests/components/RetentionPanel.test.tsx` (5 tests): size and row counts render;
  defaults (30 days / forever) when unset; select changes call `onSettingsChange` with
  `usageDetailRetentionDays: 90` / `usageRollupRetentionDays: 0` (and a non-preset 45 shows);
  "run now" calls `run_db_maintenance`, toasts the report and the table refreshes; a backend
  error is toasted.

**Verification**
- `cargo fmt --check`: clean; `cargo clippy --all-targets -- -D warnings`: clean
- `UPDATE_BINDINGS=1 cargo test --lib bindings`: 2 passed (bindings regenerated)
- `cargo test --lib`: 2452 passed, 0 failed; `cargo test --no-fail-fast`: 2653 passed, 0 failed
- `pnpm typecheck`: clean; `pnpm test:unit`: 160 files, 1547 tests passed;
  `pnpm format:check`: clean; eslint on touched files: clean;
  `node scripts/check-ipc-orphans.mjs`: 286 registered, 0 orphans
