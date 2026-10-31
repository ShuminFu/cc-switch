# Database developer guide

CC Switch keeps all of its state in one SQLite file, `~/.cc-switch/cc-switch.db`.
This page is the map for contributors: which tables exist and who owns them,
which ones travel with cloud sync, how timestamps are stored, how retention and
backups work, and the checklist for changing the schema.

The code lives in `src-tauri/src/database/`:

| File | What it holds |
|---|---|
| `mod.rs` | `Database` (main connection, read pool, usage writer, request-config cache), connection pragmas, `init()` / `memory()` / `open_file()` |
| `schema.rs` | frozen baseline DDL (v17), the `MIGRATIONS` registry and every `migrate_vN_to_vM` |
| `migration.rs` | one-time JSON (`config.json`) to SQLite import |
| `backup.rs` | SQL export/import (sync and manual), `.db` backups, restore, the import authorizer |
| `pricing_seed.rs` + `model_pricing.json` | built-in model prices as data |
| `read_pool.rs` | read-only WAL connections for dashboard queries |
| `usage_writer.rs` | batched writer thread for proxy usage rows |
| `request_cache.rs` | snapshot of the proxy request config, invalidated by the update hook |
| `tests.rs` | schema, migration, upgrade, backup/restore tests |
| `dao/providers.rs` | providers and `provider_endpoints` |
| `dao/providers_seed.rs` | official provider seeds |
| `dao/mcp.rs` | `mcp_servers` |
| `dao/prompts.rs` | `prompts` |
| `dao/skills.rs` | `skills`, `skill_repos` |
| `dao/settings.rs` | the `settings` key/value table |
| `dao/universal_providers.rs` | universal providers (a JSON value in `settings`) |
| `dao/profiles.rs` | `profiles` and the per-app current-profile pointers |
| `dao/proxy.rs` | `proxy_config`, `provider_health`, `proxy_live_backup` |
| `dao/failover.rs` | failover queue (columns on `providers`) |
| `dao/stream_check.rs` | `stream_check_logs` |
| `dao/usage_rollup.rs` | `usage_daily_rollups`, rollup and prune |

Usage statistics queries live in `src-tauri/src/services/usage_stats.rs` and the
session-log importers in `src-tauri/src/services/session_usage*.rs`.

## Connection model

- One read/write connection behind `Mutex<Connection>` (`Database.conn`, taken
  with `lock_conn!`). Pragmas: `journal_mode=WAL` (rollback journal +
  `synchronous=FULL` when WAL is unavailable or `CC_SWITCH_DISABLE_WAL=1`),
  `synchronous=NORMAL` under WAL, `foreign_keys=ON`, `busy_timeout=5s`.
- `Database::read_conn()` hands out a read-only pooled connection (WAL only;
  falls back to the main connection). Never write on it.
- Proxy usage rows go through `Database::enqueue_usage_write`, a writer thread
  that commits batches in one transaction.
- The update hook bumps the request-config generation and notifies the
  WebDAV/S3 auto-sync workers for every row change.

## Tables

| Table | Key | Synced | Owner (DAO) | Notes |
|---|---|---|---|---|
| `providers` | `(id, app_type)` | yes | `dao/providers.rs` | `settings_config`/`meta` are JSON; corrupt rows are skipped, never rewritten |
| `provider_endpoints` | `id`, unique `(provider_id, app_type, url)` | yes | `dao/providers.rs` | FK to providers, cascade delete |
| `mcp_servers` | `id` | yes | `dao/mcp.rs` | per-app `enabled_*` columns |
| `prompts` | `(id, app_type)` | yes | `dao/prompts.rs` | |
| `skills` | `id` | yes | `dao/skills.rs` | per-app `enabled_*` columns |
| `skill_repos` | `(owner, name)` | yes | `dao/skills.rs` | |
| `settings` | `key` | yes, except device keys | `dao/settings.rs` | device-local keys: `DEVICE_SETTINGS_KEYS` / `DEVICE_SETTINGS_KEY_PREFIXES` in `backup.rs` |
| `proxy_config` | `app_type` | yes, except device columns | `dao/proxy.rs` | listen address/port and takeover flags stay on the device (`PROXY_CONFIG_DEVICE_COLUMNS`) |
| `model_pricing` | `model_id` | yes | `pricing_seed.rs`, `usage_stats.rs` | `source` = `builtin` / `user` |
| `profiles` | `id` | yes | `dao/profiles.rs` | |
| `provider_health` | `(provider_id, app_type)` | no (schema only) | `dao/proxy.rs` | rebuilt at runtime |
| `proxy_request_logs` | `request_id` | no, preserved on import | usage writer, `usage_stats.rs` | detail rows, pruned after the retention window |
| `stream_check_logs` | `id` | no, preserved | `dao/stream_check.rs` | |
| `proxy_live_backup` | `app_type` | no, preserved | `dao/proxy.rs` | original live configs during takeover |
| `usage_daily_rollups` | `(date, app_type, provider_id, model, request_model, pricing_model)` | no, preserved | `dao/usage_rollup.rs` | |
| `session_log_sync` | `file_path` | no, preserved | `session_usage.rs` | per-file import cursor (absolute local paths) |
| `schema_migrations` | `version` | never exported | `schema.rs` | migration ledger of this file |

"Synced" means the rows are part of the WebDAV/S3 `db.sql` snapshot
(`SYNC_SYNCED_TABLES`). Everything else is in `SYNC_SKIP_TABLES`: its schema is
exported but not its rows; `SYNC_PRESERVE_TABLES` are copied from the live
database into the imported one, under the same lock that then replaces the
main database, so nothing written during an import is lost. A unit test
(`every_table_is_classified_for_sync`) fails when a table is in neither list.

## Timestamps

New columns use **INTEGER Unix milliseconds (UTC)**. The test
`timestamp_columns_use_known_units` in `database/tests.rs` lists every
timestamp column with its unit and fails when a new `*_at` / `*_time` column
appears without being added there, or when it is not INTEGER.

| Column | Type | Unit |
|---|---|---|
| `providers.created_at` | INTEGER | ms |
| `provider_endpoints.added_at` | INTEGER | ms |
| `prompts.created_at`, `prompts.updated_at` | INTEGER | s |
| `skills.installed_at`, `skills.updated_at` | INTEGER | s |
| `profiles.created_at`, `profiles.updated_at` | INTEGER | s |
| `proxy_request_logs.created_at` | INTEGER | s |
| `stream_check_logs.tested_at` | INTEGER | s |
| `session_log_sync.last_modified` | INTEGER | ns (rows from old versions: s) |
| `session_log_sync.last_synced_at` | INTEGER | s |
| `schema_migrations.applied_at` | INTEGER | s |
| `proxy_config.created_at`, `updated_at` | TEXT | legacy: SQLite `datetime('now')`, UTC |
| `provider_health.last_success_at`, `last_failure_at`, `updated_at` | TEXT | legacy: RFC 3339 |
| `proxy_live_backup.backed_up_at` | TEXT | legacy: RFC 3339 |
| `usage_daily_rollups.date` | TEXT | local calendar date `YYYY-MM-DD` of the machine that rolled it up |

## Money

Costs are stored as decimal TEXT (`"0.012345"`) written from `rust_decimal`.
Aggregates `CAST(... AS REAL)`; precision loss is limited to the float sum.

## Built-in pricing

`model_pricing.json` is the price list (USD per million tokens) with a
`seedVersion`. On start the seeder (`pricing_seed.rs`) runs only when this
device has not applied that version yet (device-local settings key
`model_pricing_seed_version`) or a `builtin` row is older; it then inserts new
models and refreshes `builtin` rows in one savepoint. Rows a user created or
edited are `source = 'user'` and are never overwritten. To change a price, edit
the JSON, move the old values into the model's `previous` list, and bump
`seedVersion`.

## Retention and maintenance

`Database::periodic_backup_if_needed` runs after start-up (in the background)
and daily: auto backup when due, `stream_check_logs` cleanup, cost backfill,
rollup + prune of `proxy_request_logs`, tombstone purge and
`incremental_vacuum`. `Database::init` only creates/migrates the schema and
seeds pricing. Detail rows are rolled up into `usage_daily_rollups` before they
are pruned.

## Backups and restore

- `.db` backups live in `~/.cc-switch/backups/` (owner-only), written with the
  SQLite backup API and stored with `journal_mode=DELETE` so each is one
  self-contained file. Kinds: auto, manual, safety (before an import/restore),
  pre-migration. Only auto/safety/pre-migration rotate.
- Restore stages the backup in a temp file, checks `integrity_check` and the
  schema version, drops foreign triggers/views, migrates it, then under one lock
  copies this device's live-config backups, proxy/takeover state and (by
  default) usage logs/rollups into it and replaces the main database.
- SQL import (manual or sync) executes the dump under an authorizer that only
  allows the statement shapes `dump_sql` emits (CREATE TABLE/INDEX and INSERT on
  known tables, two PRAGMAs). Orphan rows reported by `foreign_key_check` are
  removed before the swap.

## Changing the schema: checklist

1. Append `migrate_v{N-1}_to_v{N}` to `schema.rs` and a `Migration` entry at
   the end of `MIGRATIONS`. Never edit the baseline DDL or a released migration.
2. Run the tests once; `registered_migration_checksums_match_source` prints the
   checksum to register.
3. Decide `breaks_sync`: a new **table** in the snapshot breaks import on older
   clients (their authorizer rejects unknown tables), so set it; new columns on
   existing tables do not. Update the `(SCHEMA_VERSION, DB_COMPAT_VERSION)` pin
   in `schema_version_and_sync_compat_version_move_together`.
4. Classify a new table in `SYNC_SYNCED_TABLES` or `SYNC_SKIP_TABLES` (and
   `SYNC_PRESERVE_TABLES` for local data that must survive a sync import).
5. Device-local settings keys go into `DEVICE_SETTINGS_KEYS`.
6. Timestamp columns: INTEGER ms, and add them to the timestamp test.
7. Add an upgrade test. `every_historical_version_upgrades_to_the_fresh_schema`
   and `v3_8_database_upgrade_matches_fresh_schema` (an embedded v3.8 schema in
   `tests.rs`) compare migrated databases with a fresh one.
