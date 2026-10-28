# CCS-011: [db] Add a migration equivalence harness from every released schema and from legacy config.json

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p1-high`, `area:database-tests`

> **Ticket key:** `p0-migration-harness` · **Phase:** P0 · **Priority:** p1-high · **Effort:** M
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
There is no test that upgrades a real old DB. The critique notes the planned v{0..15}.sql range is wrong: pre-SQLite users upgrade from config.json through database/migration.rs `migrate_from_json`, which has no fixture.

## Evidence
- schema.rs user_version loop around :430; database/mod.rs:170 `stored_user_version_exceeds_supported`
- database/migration.rs (migrate_from_json) has no test
- database/tests.rs:130-153 holds reusable schema normalization

## Proposed change
- Build tests/fixtures/db/v{1..15}.sql from release tags. CI needs `fetch-depth: 0`; this clone has 50 commits.
- Add tests/fixtures/legacy/config.json samples (multi-app, with MCP).
- Add tests/migrations.rs, which asserts:
  - user_version == SCHEMA_VERSION
  - normalized sqlite_master/table_info match a fresh DB
  - row data is preserved
  - a second migration is a no-op
  - a vN+1 DB is rejected by the vN app

## Acceptance criteria / verification
- [ ] At least 10 distinct start versions plus 2 legacy JSON fixtures pass
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test migrations`
- [ ] A script (scripts/gen-db-fixture.sh) documents how to regenerate fixtures from a tag

## Dependencies
None.
