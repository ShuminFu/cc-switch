# CCS-086: [db] Block ATTACH, VACUUM INTO and unsafe statements during SQL import and sync restore

**Status:** Done · **Kind:** security · **Labels:** `security`, `phase:P1`, `p1-high`, `area:database`

> **Ticket key:** `p1-sql-import-sandbox` · **Phase:** P1 · **Priority:** p1-high · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding backend-sql-import-attach, partially real. Import runs execute_batch on arbitrary SQL behind a header check that is easy to fake. ATTACH or VACUUM INTO can create SQLite files at any writable path and tamper with other SQLite DBs, such as browser profiles. The sync SHA256 comes from the same remote, so it does not authenticate anything.

## Evidence
- src-tauri/src/database/backup.rs:94-122, ~170-181 (validate_cc_switch_sql_export); commands/import_export.rs:50; services/sync_protocol.rs:270-303,326

## Proposed change
- On temp_conn, set SQLITE_LIMIT_ATTACHED to 0 (rusqlite limits feature or raw sqlite3_limit).
- Install an authorizer (hooks is enabled) that denies ATTACH, DETACH, VACUUM, CREATE TRIGGER/VIEW, and every PRAGMA outside a small allowlist.
- Optionally accept only CREATE TABLE/INDEX and INSERT statements.

## Acceptance criteria / verification
- [x] Tests: header + `ATTACH DATABASE '<tmp>/pwned.db' AS x; CREATE TABLE x.t(a)` returns Err and pwned.db does not exist; `VACUUM INTO '<tmp>/v.db'` gives the same result
- [x] A legitimate export round-trips (existing tests)
- [x] `cargo test --manifest-path src-tauri/Cargo.toml database::backup`

## Dependencies
None.

## Implementation notes
- **Change:** `import_sql_string_inner` (shared by file import, `import_sql_string` and the WebDAV sync restore `import_sql_string_for_sync`) now hardens the temp connection before `execute_batch`:
  - `set_limit(SQLITE_LIMIT_ATTACHED, 0)` (rusqlite `limits` feature) — rejects `ATTACH` and `VACUUM INTO` (which attaches its output internally) before any file is opened.
  - An authorizer (`Database::authorize_sql_import`) active only for the import batch allows just what `dump_sql` emits: transactions, `CREATE TABLE`/`CREATE INDEX`, `INSERT`, reads/functions, SQLite's internal `sqlite_master` update and `REINDEX` from index creation, and the `foreign_keys`/`user_version` PRAGMAs. Everything else (ATTACH/DETACH, triggers, views, temp objects, DELETE/UPDATE/DROP/ALTER, virtual tables, other PRAGMAs such as `writable_schema`) is denied. The authorizer is removed before schema completion/migrations run.
- **Files:** `src-tauri/Cargo.toml` (rusqlite `limits` feature), `src-tauri/src/database/backup.rs` (sandbox + regression test `sql_import_rejects_attach_vacuum_into_and_unsafe_statements`).
- **Commits:** 6a58d0e (code + test); ticket notes in the follow-up docs commit.
- **Verification:**
  - New test checks header + `ATTACH DATABASE '<tmp>/pwned.db' AS x; CREATE TABLE x.t(a)` and `VACUUM INTO '<tmp>/v.db'` (plus trigger, view, `PRAGMA writable_schema`, `DELETE`) return Err via both normal and sync import, that neither file exists, that the main DB is untouched, and that a real export still round-trips. With the two sandbox lines disabled the test fails (it panics on the first assertion); with them in place it passes.
  - `cargo test --manifest-path src-tauri/Cargo.toml --lib database::backup` passes: 5 passed, 0 failed (the existing export/import and sync round-trip tests included).
  - `cargo fmt --check` is clean. `cargo clippy -- -D warnings` is clean.
  - `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: 2398 passed, 0 failed.
