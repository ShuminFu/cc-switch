# CCS-156: [db] Replace per-app MCP and skill columns with join tables, dual-writing the old columns for one release

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p2-medium`, `area:database`

> **Ticket key:** `p4-join-tables` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** L · **Depends on:** `p4-migration-framework`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Evidence
- dao/mcp.rs, dao/skills.rs (per-app columns, positional row.get); the proxy_config CHECK constraint

## Proposed change
- Add a migration: mcp_server_apps and skill_apps join tables, and drop the proxy_config CHECK.
- The DAOs stop naming apps and use named columns.
- Keep writing the old columns for one release so mixed-version WebDAV/S3 sync works, and bump DB_COMPAT_VERSION per policy.
- AppSet replaces McpApps and SkillApps.

## Acceptance criteria / verification
- [x] The migration harness passes; an older-version fixture importing the new dump keeps its app flags
- [x] A test-only 9th app needs no migration
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --test migrations dao::`

## Dependencies
p4-migration-framework

## Implementation notes

**Summary**
- **Schema v17** (`database/migrations/v017.rs`, `DbCompatImpact::Additive`):
  - Adds `mcp_server_apps(server_id, app_type)` and `skill_apps(skill_id, app_type)`. Each table has a composite primary key and one row per enabled app.
  - Backfills both tables from the existing `enabled_<slug>` columns. A column that is missing is skipped, so partial older schemas still work.
  - Rebuilds `proxy_config` with the same columns and rows but without `CHECK (app_type IN (...))`. `SCHEMA_VERSION` is now 17.
- **`AppSet` replaces `McpApps` and `SkillApps`** (`app_config.rs`):
  - It holds a set of `AppType` values in registry order, and accepts only apps that have the MCP or SKILLS caps.
  - Serde keeps the old flat JSON shape (`{"claude":true,…,"hermes":false}`, with keys taken from the registry). Reads accept the old flat keys and aliases, and ignore unknown keys. The frontend, `config.json` and deep links are unchanged.
  - Every caller has moved to `is_enabled_for` / `set_enabled_for` / `from_apps` / `only` / `union`. `McpService::upsert_server` now loops over the previous set instead of six hand-written `if`s.
- **DAOs** (`dao/mcp.rs`, `dao/skills.rs`, plus the new `dao/app_sets.rs`):
  - They no longer name apps and use named-column `row.get("…")`. Flags are read only from the join tables.
  - Writes go through `AppJoinTable::write` inside a savepoint. It replaces the join rows and dual-writes the frozen legacy columns (`LEGACY_FLAG_APPS`) for one release.
  - A save replaces only rows for apps this build knows. A row that a newer build of the same schema wrote for a new app is kept. Delete and clear also remove the join rows.
  - The JSON → SQLite import (`migration.rs`) uses the same save path.
- **`DB_COMPAT_VERSION`**: under docs/dev/DB_COMPATIBILITY.md the step is additive, so it is **not** bumped and stays at 6. The step adds new tables and relaxes a constraint. Every existing column keeps its meaning and is still written, so a build of the same generation can still read the old columns of a v17 dump.
  - Dropping the legacy columns in a later release is a non-additive change and must bump `DB_COMPAT_VERSION` then.
- **Fixtures**:
  - `scripts/gen-db-fixture.sh` now also seeds join-table rows when the tables exist, matching the sample `enabled_*` flags.
  - The new fixture `tests/fixtures/db/v17.sql` was generated with `CCS_FIXTURE_LOCKFILE=current` from a temporary commit with an identical `src-tauri/src` tree (tree `3cc9a2b`). Its header points at `dd64eeb`.

**Acceptance**
- The migration harness passes for all 16 DB fixtures (v1–v13, v15–v17), plus v0 and the 2 legacy JSON fixtures. Older-version flags are preserved:
  - `dao::every_fixture_keeps_mcp_and_skill_app_flags`: for every fixture, the DAO flags after upgrade equal the fixture's `enabled_*` columns.
  - `dao::older_readers_of_a_new_dump_keep_app_flags`: the v16 DAO's raw `SELECT enabled_*` on a v17 dump returns the same flags. A database upgraded from the v16 fixture keeps them after importing that dump.
  - `dao::new_build_importing_an_older_dump_keeps_app_flags`: a v16-shaped dump imported through `import_sql_string` is backfilled.
- A test-only 9th app needs no migration. `dao::ninth_app_needs_no_migration` inserts `ninth` rows into `mcp_server_apps`, `skill_apps` and `proxy_config` without any DDL. After that:
  - the schema and `user_version` are unchanged;
  - DAO reads ignore the unknown app, and saves keep its rows;
  - the rows survive an export and import round trip.
  - The unit test `rows_for_apps_this_build_does_not_know_survive_a_save` covers the same save behaviour.
- `cargo test --manifest-path src-tauri/Cargo.toml --test migrations dao::`: 4 passed.

**Files**
- `src-tauri/src/app_config.rs`
- `src-tauri/src/apps/registry.rs`
- `src-tauri/src/commands/mcp.rs`
- `src-tauri/src/database/{mod.rs,migration.rs}`
- `src-tauri/src/database/dao/{app_sets.rs,mcp.rs,skills.rs,mod.rs}`
- `src-tauri/src/database/migrations/{mod.rs,v017.rs,tests.rs}`
- `src-tauri/src/deeplink/{mcp.rs,tests.rs}`
- `src-tauri/src/lib.rs`
- `src-tauri/src/mcp/common.rs`
- `src-tauri/src/services/{mcp.rs,skill.rs}`
- `src-tauri/tests/{migrations,import_export_sync,live_write_preview,mcp_commands,profile_roundtrip,provider_commands,provider_service,skill_sync}.rs`
- `src-tauri/tests/fixtures/db/v17.sql`
- `scripts/gen-db-fixture.sh`

**Commits**
- `dd64eeb`: code, tests and fixture.
- This ticket update follows as a docs commit, together with the v17 fixture header pointing at `dd64eeb`.

**Verification**
- `cargo fmt --check`: clean. `cargo clippy --all-targets -- -D warnings`: clean.
- `cargo test --test migrations`: 13 passed, 0 failed. The `dao::` filter: 4 passed.
- `cargo test --lib`: 2375 passed, 0 failed, 2 ignored. This includes the new tests:
  - `migrate_v16_to_v17_backfills_join_tables_and_drops_proxy_config_check`
  - `migrate_v16_to_v17_backfill_tolerates_missing_legacy_columns`
  - the `app_sets::tests` dual-write tests
  - the `AppSet` serde shape tests
- Full suite `cargo test --no-fail-fast`: 2573 passed, 0 failed.
- Before the v17 fixture was added, the only failure was `db_fixtures_cover_every_schema_version` (a v17 fixture was missing).

**Review.** I checked each acceptance criterion against `dd64eeb` and `4671acb`: the v17 step, the DAOs (named columns, flags read only from the join tables, legacy dual-write inside a savepoint), the `AppSet` JSON shape, the v17 fixture and the four `dao::` tests. I grepped for remaining `McpApps`/`SkillApps` users and for raw `enabled_*` SQL outside the DAOs; there are none. I re-ran fmt, clippy `--all-targets`, `--test migrations` (13 passed) and the full `cargo test --no-fail-fast`. Leaving `DB_COMPAT_VERSION` at 6 is consistent with docs/dev/DB_COMPATIBILITY.md: the data is copied, not moved, the old columns keep their meaning, and an older build refuses a v17 dump anyway. One gap fixed: `AppJoinTable::write` replaced rows for every app in the registry, including apps that `AppSet` cannot hold (OpenClaw, Claude Desktop). If a newer build of the same schema gives one of these apps MCP or Skills support, a save from this build would silently delete its rows. The write now replaces only rows for `AppSet::supported_apps()`. `rows_for_apps_this_build_does_not_know_survive_a_save` now also covers an `openclaw` row. The acceptance boxes are now ticked.
