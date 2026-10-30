# CCS-159: [db] Add usage and session-cursor repositories with a UsageQuery builder and transactional ingest

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p2-medium`, `area:database`

> **Ticket key:** `p4-usage-repo` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** L · **Depends on:** `p4-migration-framework`, `p1-session-sync-final-entry`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
- Add database/repo/{usage,pricing,session_cursor}.rs, a UsageQuery builder with keyset pagination, and a UsageSource enum.
- Session ingest writes one transaction per file together with its cursor.
- Add a CI grep: no SQL outside database/.

## Acceptance criteria / verification
- [x] Test: kill the process between files during import, re-run, and there is no double counting
- [x] The CI grep for `SELECT |INSERT |UPDATE ` outside src-tauri/src/database passes
- [x] `cargo test --manifest-path src-tauri/Cargo.toml repo::`

## Dependencies
p4-migration-framework, p1-session-sync-final-entry

## Implementation notes

**Summary.** The app-database SQL that lived outside `database/` now lives in
`src-tauri/src/database/repo/`:

- `repo/usage.rs`: the Dashboard queries and zero-cost backfill (moved verbatim
  out of `services/usage_stats.rs`, which now only holds the DTOs and re-exports
  the dedup helpers), the `UsageSource` enum (`data_source` values, placeholder
  provider ids, session display names, now used to generate the provider-name
  CASE and the session `IN (...)` list), the `UsageQuery` builder (app / provider
  name / model / status / time / source filters, `count`, `fetch` with keyset
  pagination on `(created_at, request_id)` plus OFFSET for the page-number
  IPC), and the write paths: `insert_proxy_row` (proxy usage logger),
  `insert_session_row` / `upsert_session_row` / `upgrade_session_row` /
  `session_row_output_tokens` (session ingest), `data_source_breakdown`.
  `get_request_logs` goes through `UsageQuery` (order is now
  `created_at DESC, request_id DESC`, which is deterministic).
- `repo/pricing.rs`: `model_pricing` statements from `pricing/catalog.rs`,
  `pricing/resolver.rs` and the `get_model_pricing` command. The catalog and
  resolver keep their rules and call the repo.
- `repo/session_cursor.rs`: `session_log_sync` get/put and the Codex
  archived-file cursor lookup.
- Transactional ingest: `services::session_usage::ingest_file` opens one
  transaction per source file (per OpenCode session), writes the rows and the
  cursor, and commits them together. Row-level insert errors are still logged and
  skipped. A cursor write or commit failure rolls back the whole file. Claude,
  Codex, Gemini and OpenCode parsers parse first, then write in that transaction.
- No-SQL guard: `database::repo::tests::no_sql_outside_database` scans
  `src-tauri/src/**.rs` outside `database/` for `SELECT |INSERT |UPDATE `,
  skipping comments, trailing `#[cfg(test)] mod tests {}` blocks and
  `#[cfg(test)] mod x;` files. Files that touch *other apps'* SQLite databases
  (Codex/Hermes/OpenCode session readers, OpenCode usage reader, Codex history
  migration) and the e2e `proxy_harness.rs` are allowlisted with a reason, and a
  stale entry fails the test. It runs in CI's backend job through
  `cargo test`, so no workflow file had to change (that lane belongs to another
  agent).
- Not in scope here (other P4 rows of the plan): `superseded_by` column, micro-USD
  rollups, `RetentionPolicy`. `database/schema.rs` keeps the pricing save/delete
  statements it already had (they were already inside `database/`).

**Files.** `src-tauri/src/database/repo/{mod,usage,pricing,session_cursor,tests}.rs`
(new), `database/mod.rs`, `services/{usage_stats,session_usage,session_usage_codex,session_usage_gemini,session_usage_opencode}.rs`,
`pricing/{catalog,resolver}.rs`, `commands/usage.rs`, `proxy/usage/logger.rs`
(only the insert call changed).

**Commits.** cb171bc (code), plus the follow-up commit that records these notes.

**Verification.**
- `cargo test --manifest-path src-tauri/Cargo.toml --lib repo::`: 11 passed. Covers
  `session_import_killed_between_files_does_not_double_count`, which simulates a
  crash at b.jsonl's cursor write with a temp trigger and re-runs the import.
  It **fails** when the cursor is committed separately from the rows (checked by
  temporarily reverting `ingest_file`: left `(3, 603)` vs right `(0, 0)`) and
  passes with the transaction.
- `no_sql_outside_database` passes (the CI grep criterion).
- `cargo fmt --check`: clean. `cargo clippy --all-targets -- -D warnings`: clean.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: lib 2397 passed,
  0 failed, 2 ignored. All integration test binaries passed.
