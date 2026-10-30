# CCS-171: [usage] Add a usage re-scan action and a per-source data-quality report

**Status:** Done · **Kind:** feature · **Labels:** `feature`, `phase:P4`, `p3-low`, `area:usage`

> **Ticket key:** `p4-usage-rescan` · **Phase:** P4 · **Priority:** p3-low · **Effort:** M · **Depends on:** `p4-usage-repo`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Backend idea 7. Existing rows that were undercounted before the P1 fixes cannot be repaired.

## Proposed change
- Add a "re-scan session logs" action that resets the cursors and re-imports from offset 0 with upsert semantics, in the background with progress.
- Add a data-quality report via get_data_source_breakdown.

## Acceptance criteria / verification
- [x] Test: a DB with an undercounted row is corrected after a re-scan, with no duplicates
- [x] `cargo test --manifest-path src-tauri/Cargo.toml session_usage`

## Dependencies
p4-usage-repo

## Implementation notes

**Summary.** A re-scan mode (`SyncMode::Rescan`) was added to every session importer (Claude JSONL, Codex JSONL, Gemini JSON, OpenCode DB). In this mode the stored cursors are ignored, so each source is re-read from offset 0. Each source's cursor is rewritten in its own file transaction once that source is scanned. Using cursor-ignore instead of a separate `DELETE` means a periodic incremental sync running at the same time cannot move a cursor forward and cause a file to be skipped. Rows are written with upsert semantics keyed by `request_id`:
- `usage_repo::repair_session_row` rewrites an existing row from the same source when the re-parsed token total (input + output + cache_read + cache_creation) is larger. It recomputes costs and `pricing_model`. Values only go up: smaller re-parsed values, such as an early snapshot of the same message in another file, never lower a row.
- Messages that are missing go through the normal dedup and insert path. They are skipped when they are older than the daily-rollup horizon (`usage_repo::rollup_horizon`, which is local midnight after the newest `usage_daily_rollups` date). Those details were already pruned and folded into rollups, so inserting them again would count them twice.
- `rescan_all_session_usage` runs Claude → Codex → Gemini → OpenCode. After each source it reports `SessionRescanProgress`, emitted as the `session-usage-rescan-progress` event. Only one re-scan can run at a time. A failure in one source is recorded in `errors` and the remaining sources still run. The new `rescan_session_usage` command runs it on the blocking pool.
- Data-quality report: `get_usage_data_sources` (`get_data_source_breakdown`) now returns, per source, `duplicateCount` (session rows superseded by proxy logs), `unpricedCount` (uses the same definition as the unpriced filter), `missingSessionCount`, `zeroOutputCount`, and `firstCreatedAt`/`lastCreatedAt`, along with the existing count and cost of effective rows.
- UI: the usage data-source bar has a "Re-scan" button with per-source progress text and a result toast. Each source chip shows a data-quality tooltip and a warning icon when it has unpriced or zero-output rows. i18n strings were added for en, zh, zh-TW and ja.

**Files.** `src-tauri/src/services/session_usage{,_codex,_gemini,_opencode}.rs`, `src-tauri/src/database/repo/usage.rs`, `src-tauri/src/commands/usage.rs`, `src-tauri/src/usage_events.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/bindings.rs`, `src/shared/bindings.ts` (regenerated), `src/lib/api/usage.ts`, `src/types/usage.ts`, `src/components/usage/DataSourceBar.tsx`, `src/i18n/locales/{en,zh,zh-TW,ja}.json`, `tests/components/DataSourceBar.test.tsx` (new), `tests/lib/usageBindings.test.ts`, `tests/msw/unmockedCommands.ts`.

**Commits.** `1bddf77` feat(usage): add a session-log re-scan action and per-source data-quality report (CCS-171); this notes commit follows it.

**Tests.**
- `session_usage::tests::test_rescan_repairs_undercounted_row_without_duplicates`: an old row is missing `cache_creation` and its cursor is at EOF. Incremental sync, even after the cursor is reset to 0, leaves the row undercounted. The re-scan repairs tokens and cost, keeps one row per message, rewrites the cursor, and a second re-scan changes nothing.
- `test_rescan_never_downgrades_and_skips_rolled_up_period`: checks the rollup horizon value, that rolled-up periods are not re-inserted, and that the re-scan never lowers a row.
- `test_rescan_sources_reports_progress_and_rejects_concurrent_runs`: checks the progress events, that one source failing does not stop the others, and the concurrency guard.
- `test_data_source_breakdown_reports_quality`: checks the quality fields.
- `session_usage_codex::tests::test_rescan_repairs_undercounted_codex_row`: checks repair for a source other than Claude.
- `tests/components/DataSourceBar.test.tsx`: checks the tooltip and warning flag, and that the re-scan button calls `rescan_session_usage`.

**Verification.**
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`: clean.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: clean. With `--all-targets` it is also clean.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib session_usage`: 56 passed, 0 failed.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 2466 passed, 0 failed, 3 ignored. This includes the `bindings_are_up_to_date` check after `UPDATE_BINDINGS=1` regeneration.
- `pnpm typecheck`: clean.
- `pnpm test:unit`: all passed after updating the usage-binding command count to 15. Re-run checks: `usageBindings` 2/2 and `DataSourceBar` 2/2.
- `pnpm format:check`: clean.

**Review.** An independent review checked both acceptance criteria against `1bddf77`. Repaired rows stay consistent with cross-source dedup, because the `superseded_by` triggers fire on token-column updates. The rollup horizon matches the local-midnight cutoff that `rollup_and_prune` uses, and every importer's cursor read goes through `SyncMode::sync_state`. No functional defects were found. One test gap was closed: `test_data_source_breakdown_reports_quality` never tested a non-zero `duplicateCount`. It now adds a session row covered by a proxy log and checks three things: the row counts only as a duplicate, it is left out of the other quality counts and the time range, and the proxy source shows no duplicates. Known minor limitation, not fixed: the progress listener in `DataSourceBar` is attached after the re-scan request is sent. The first progress event can therefore be missed, though the result toast is unaffected. Re-verified: `cargo fmt --check` clean; `cargo clippy -- -D warnings` clean; `cargo test --lib session_usage` 56/56; `cargo test --lib` 2466 passed, 0 failed; `pnpm typecheck` clean; `DataSourceBar` and `usageBindings` vitest 4/4; prettier clean on the touched frontend files.
