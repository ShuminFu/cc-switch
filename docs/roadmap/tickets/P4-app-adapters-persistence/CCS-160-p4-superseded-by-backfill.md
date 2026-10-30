# CCS-160: [db] Replace the correlated EXISTS with a superseded_by column and a chunked, resumable backfill

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p2-medium`, `area:database`

> **Ticket key:** `p4-superseded-by-backfill` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p4-usage-repo`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Dashboard queries run a correlated EXISTS. Per the critique, a startup backfill on a large proxy_request_logs table would block startup for a long time.

## Evidence
- usage_stats.rs:297-336

## Proposed change
- Add the superseded_by column.
- Run the backfill in the background in 10k-row chunks with a persisted cursor and progress events. It resumes after a crash, and queries fall back to EXISTS until it completes.

## Acceptance criteria / verification
- [x] On a 1M-row synthetic DB: startup is not blocked; the dashboard summary p95 is 50 ms or less after the backfill; results equal the EXISTS logic (parity test)
- [x] A kill mid-backfill resumes from the cursor
- [x] `cargo test --manifest-path src-tauri/Cargo.toml superseded`

## Dependencies
p4-usage-repo

## Implementation notes

**Summary.** Schema v19 (`database/migrations/v019.rs`, `DbCompatImpact::Additive`, `DB_COMPAT_VERSION` unchanged) adds `proxy_request_logs.superseded_by TEXT`. The column holds the `request_id` of a 2xx proxy row that covers a session row; NULL means the row counts. The step also adds a partial index `idx_request_logs_superseded_by` (`WHERE superseded_by IS NOT NULL`), the local `usage_backfill_state` table (`name, cursor, processed, done, updated_at`) and three triggers. The migration does no backfill. An empty table is marked done right away. Otherwise `done = 0`, so startup is not blocked.

- **Triggers keep the column current** for every write path, including session imports, raw test INSERTs and sync restores. They use the same match predicate as the old EXISTS (`repo/superseded.rs::proxy_match_conditions`).
  - `trg_request_logs_superseded_ai`: computes a new session row, marks NULL session rows that a new proxy row covers, and recomputes rows that point at the same `request_id`. That last step handles `INSERT OR REPLACE`, which fires no DELETE trigger.
  - `trg_request_logs_superseded_au`: on `UPDATE OF` the fingerprint columns, recomputes the row and the rows that point at it. `superseded_by` is not in the column list, so the trigger never fires itself.
  - `trg_request_logs_superseded_ad`: when a proxy row is deleted (rollup pruning), recomputes the rows that pointed at it. This keeps results equal to EXISTS.
- **Background backfill.** `Database::run_superseded_backfill` is started with `spawn_blocking` from `lib.rs` setup.
  - It walks `request_id` order in chunks of `BACKFILL_CHUNK_ROWS = 10_000`.
  - Each chunk is one SAVEPOINT that also advances the cursor and `processed`, so a kill loses at most the chunk that was in progress. The next start resumes from the cursor.
  - The DB lock is released and the task pauses 20 ms between chunks.
  - It emits `usage-backfill-progress` (`{processed, total, done}`) after each chunk through `usage_events::notify_backfill_progress`.
- **Query switch.** `effective_usage_log_filter(conn, alias)` returns `alias.superseded_by IS NULL` once the state row is done. Before that, or when the DB has no state table, it falls back to `superseded_exists_filter` (the old EXISTS, unchanged). All callers now pass the connection: the dashboard queries, `UsageQuery`, rollup and `data_source_breakdown`.
- **Export, import and sync.**
  - `dump_sql` no longer exports triggers. The import authorizer would reject `CREATE TRIGGER`, and no triggers existed before this change.
  - `apply_schema_migrations_on_conn` recreates the triggers with `ensure_triggers` after each migration run, which covers importing a v19 dump.
  - `usage_backfill_state` is in `SYNC_SKIP_TABLES` and `SYNC_PRESERVE_TABLES`, so it stays with the local `proxy_request_logs`.
  - During a sync import, the triggers are dropped while the local tables are restored and then recreated. The restored values are already consistent, so no rows are recomputed.
  - `usage_backfill_state` does not advance the config generation.

**Remaining limitation.** A v18 SQL dump or backup restored while the app is running starts in fallback mode (`done = 0`). Results are still correct. The backfill completes at the next app start.

**Files.**
- `src-tauri/src/database/repo/superseded.rs` (new: predicate, triggers, backfill, tests)
- `src-tauri/src/database/migrations/v019.rs` (new)
- `src-tauri/src/database/migrations/mod.rs`
- `src-tauri/src/database/mod.rs` (`SCHEMA_VERSION = 19`)
- `src-tauri/src/database/repo/mod.rs`
- `src-tauri/src/database/repo/usage.rs`
- `src-tauri/src/database/backup.rs`
- `src-tauri/src/database/dao/usage_rollup.rs`
- `src-tauri/src/services/usage_stats.rs`
- `src-tauri/src/usage_events.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/tests/fixtures/db/v19.sql` (new)

**Fixture.** I generated `v19.sql` with `CCS_FIXTURE_LOCKFILE=current scripts/gen-db-fixture.sh` from a temporary dangling commit (`62f5bb3`). Its `src-tauri/src` tree is identical to `7e12043`. The header points at `7e12043`.

**Commits.** `7e12043` (code, tests, fixture). A follow-up commit holds this note and the fixture header.

**Verification.**
- `cargo test --manifest-path src-tauri/Cargo.toml superseded`: 6 passed, 1 ignored (the benchmark). The tests are in `database::repo::superseded::tests`:
  - Fresh-DB trigger parity against EXISTS on 3k synthetic rows, with more than 100 rows deduplicated.
  - Parity after deletes, updates and `INSERT OR REPLACE`.
  - The backfill gives the same results and the same `get_usage_summary`, and queries switch from EXISTS to the column.
  - `superseded_backfill_resumes_from_cursor_after_kill`: on a file DB, two chunks commit, then a third runs inside an uncommitted transaction and the connection is dropped. On reopen, `processed` is 1000, the cursor is `r-0000999` and no rows past the cursor are filled. The resumed run processes exactly the remaining rows, and parity holds.
  - A v18 → v19 migration defers the backfill, and rows written during the backfill stay correct.
  - Full SQL export and import, plus sync import, keep the triggers, state and values.
- `cargo test ... superseded_benchmark -- --ignored --nocapture` on a 1M-row synthetic v18 DB spread over 30 days (dev profile, unoptimized):
  - v18 → v19 migration (startup work): 225 ms.
  - Backfill of 1,000,000 rows in the background: 32.3 s.
  - Dashboard summary for the default "today" range: p95 42.0 ms before the backfill (EXISTS), 24.0 ms after. That meets the ≤ 50 ms target, and parity holds on 1M rows.
  - Summary over all 30 days: 636 ms. That time is the aggregate scan, not deduplication.
- `cargo test --manifest-path src-tauri/Cargo.toml --test migrations`: 14 passed (includes the new v19 fixture).
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: lib 2407 passed, 0 failed, 3 ignored. All integration suites passed. The only failure before the fixture was added was `db_fixtures_cover_every_schema_version`, and it passes now.
- `cargo fmt`: clean. `cargo clippy --all-targets -- -D warnings`: clean.

**Review.** An independent review checked each acceptance criterion against the code and re-ran the verification: `superseded` tests, the 1M-row benchmark (migration 222 ms, backfill 31.3 s, "today" summary p95 43.5 ms before and 24.1 ms after the backfill), the full `cargo test --no-fail-fast` (lib 2408 passed, 0 failed), `cargo fmt` and `cargo clippy --all-targets -- -D warnings`. The predicate, all call sites, the sync and import paths, and resume after a kill are correct. Two follow-ups were fixed:
- On every proxy write, the insert and update triggers looked up session rows by fingerprint without a `created_at` range, because the window was written as `NEW.created_at BETWEEN column ± window`. That scanned every historical session row with the same token tuple. The step now adds the equivalent `proxy_request_logs.created_at BETWEEN NEW.created_at ± window`, so the lookup uses the `created_at` range of `idx_request_logs_dedup_lookup_expr`. Results are unchanged, and parity tests cover it.
- `ensure_triggers` used `CREATE TRIGGER IF NOT EXISTS`, so a database that already had the triggers (from the first v19 build or from the `v19.sql` fixture) kept the old definition. It now drops the triggers and recreates them. The new test `superseded_triggers_are_recreated_with_current_definition` covers this.
