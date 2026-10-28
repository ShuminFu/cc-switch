# CCS-086: [db] Block ATTACH, VACUUM INTO and unsafe statements during SQL import and sync restore

**Status:** Open · **Kind:** security · **Labels:** `security`, `phase:P1`, `p1-high`, `area:database`

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
- [ ] Tests: header + `ATTACH DATABASE '<tmp>/pwned.db' AS x; CREATE TABLE x.t(a)` returns Err and pwned.db does not exist; `VACUUM INTO '<tmp>/v.db'` gives the same result
- [ ] A legitimate export round-trips (existing tests)
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml database::backup`

## Dependencies
None.
