# CCS-011: [db] Add a migration equivalence harness from every released schema and from legacy config.json

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p1-high`, `area:database-tests`

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
- [x] At least 10 distinct start versions plus 2 legacy JSON fixtures pass
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --test migrations`
- [x] A script (scripts/gen-db-fixture.sh) documents how to regenerate fixtures from a tag

## Dependencies
None.

## Implementation notes

**Summary.** Added `src-tauri/tests/migrations.rs`. It loads every DB fixture into a `TestEnv` home, runs the real `Database::init()` and checks:
- `user_version == SCHEMA_VERSION`
- the normalized schema (columns/type/notnull/default/pk, indexes, foreign keys, CHECK constraints) matches a fresh DB
- every fixture row in the user-data tables is preserved
- a second `init()` changes nothing: same schema and data, and no new pre-migration backup
- a `SCHEMA_VERSION + 1` DB is flagged by `stored_user_version_exceeds_supported`, refused by `init()` and left untouched

Two legacy `config.json` samples go through `MultiAppConfig::load()` + `migrate_from_json()`:
- v3.7: unified MCP, three apps, custom endpoints, prompts, skill repos, common config
- v3.6: per-app `mcp.claude`/`mcp.codex`, no gemini section, legacy `claude_common_config_snippet`

Both are checked for schema equality, specific migrated rows and a no-op second init. A coverage test fails when a schema version up to `SCHEMA_VERSION` has no fixture. `SCHEMA_VERSION` is now `pub` and re-exported from `lib.rs` so integration tests can use it.

**Fixtures.** `scripts/gen-db-fixture.sh <ref>` produced 14 start versions (v1–v13 and v15). For each ref it builds the old crate in a temporary worktree, runs that release's own `Database::init()` with a redirected HOME, seeds sample rows filtered to the columns that exist in that schema, and dumps the result as SQL with `user_version`. Sources, as recorded in each fixture header:

| Version | Source |
|---|---|
| v1 | v3.8.3 |
| v2 | v3.9.0-3 |
| v3 | v3.9.1 |
| v4 | v3.10.2 |
| v5 | v3.11.1 |
| v6 | v3.12.3 |
| v7 | 2d581bce91 |
| v8 | v3.13.0 |
| v9 | 81af0a57f9 |
| v10 | v3.16.2 |
| v11 | v3.16.5 |
| v12 | 06039540ff |
| v13 | v3.17.0 |
| v15 | 1c0ee0c5 |

Versions without a release tag come from the upstream commit that introduced them.

**v14 has no fixture on purpose** (`VERSIONS_WITHOUT_FIXTURE`):
- Upstream briefly had a different v14 (13e7c1fc / ac52c851, cache-write TTL columns). It was reverted in 6eb217b2 and never released.
- The current v14 was introduced together with v15 and never existed on its own.

Fixture generation needs full history and tags; this clone has none, so they were generated from a full upstream clone via `CCS_FIXTURE_GIT_DIR`. The tests themselves only read the committed fixtures, so CI does not need `fetch-depth: 0` unless it regenerates fixtures.

**Files**
- `src-tauri/tests/migrations.rs`
- `src-tauri/tests/fixtures/db/v{1..13,15}.sql`
- `src-tauri/tests/fixtures/legacy/config-v3.6-per-app-mcp.json`
- `src-tauri/tests/fixtures/legacy/config-v3.7-unified-mcp.json`
- `scripts/gen-db-fixture.sh`
- `src-tauri/src/database/mod.rs`
- `src-tauri/src/lib.rs`

**Commits:** 3aae254 (harness, fixtures, script), plus this ticket update.

**Verification**
- `cargo test --manifest-path src-tauri/Cargo.toml --test migrations`: 5 passed, 0 failed.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 2078 passed, 0 failed, 3 ignored.
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`: clean.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`: clean.
- `bash -n scripts/gen-db-fixture.sh`: OK (shellcheck is not installed).
- Every `gen-db-fixture.sh` run logged `wrote …/v<N>.sql (schema v<N>)`.

**Acceptance criteria**
- [x] At least 10 distinct start versions (14) plus 2 legacy JSON fixtures pass
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --test migrations`
- [x] `scripts/gen-db-fixture.sh` documents and performs fixture regeneration from a tag
