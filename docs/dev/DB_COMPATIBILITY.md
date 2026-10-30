# Database compatibility policy

This page says what CC Switch does when two builds disagree about the database
schema: a downgrade or channel switch on one machine, and devices running
different versions that share a sync remote. It also says when to bump each
version constant. Anyone changing the database schema or the
sync format should read it first. Migrations live in
`src-tauri/src/database/migrations/`. The PR template has a checkbox for it.

## Version numbers

| Constant                   | Where                                     | Meaning                                                                                                                                                 |
| -------------------------- | ----------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `SCHEMA_VERSION`           | `src-tauri/src/database/mod.rs`           | The SQLite `user_version` this build migrates to. Every `.db` file, backup and SQL dump carries one (dumps write `PRAGMA user_version=N;`).             |
| `DB_COMPAT_VERSION`        | `src-tauri/src/services/sync_protocol.rs` | The sync generation of `db.sql`. It is part of the remote path (`<root>/v2/db-v6/<profile>/`) and of `manifest.json` (`dbCompatVersion`).               |
| `LEGACY_DB_COMPAT_VERSION` | same file                                 | The generation assumed for the old remote layout (no `db-vN` path segment) whose manifests have no `dbCompatVersion`. It is frozen at 5. Never bump it. |
| `PROTOCOL_VERSION`         | same file                                 | The manifest/artifact wire format. It is separate from the database and out of scope here.                                                              |

## Rules for schema changes

1. **Forward migration only.** A build opens any database with
   `user_version <= SCHEMA_VERSION` and migrates it up, one step at a time,
   through the ordered `MIGRATIONS` table in `database/migrations/mod.rs`. A new
   database starts at v0 and runs every step, beginning with the frozen v1
   baseline. Never remove or edit a released step, and never repair the schema
   outside a step: errors propagate and roll the whole run back. Each applied
   step is recorded in the device-local `schema_migrations` table (not synced).
   `tests/migrations.rs` replays every `tests/fixtures/db/v{N}.sql`, and a unit
   test checks that `MIGRATIONS` is contiguous and ends at `SCHEMA_VERSION`.
2. **Bump `SCHEMA_VERSION` for every change to the stored schema.** Append a
   `Migration { version: N + 1, .. }` step and a fixture made with
   `scripts/gen-db-fixture.sh`. The step declares its `db_compat` impact:
   `DbCompatImpact::Additive`, or `DbCompatImpact::Breaking { db_compat_version }`
   together with the `DB_COMPAT_VERSION` bump described below. `PrePolicy` marks
   the non-additive steps released before this policy (v15 and earlier) and is
   rejected for new steps.
3. **Keep changes additive within one `DB_COMPAT_VERSION`.** That means new
   tables, new columns with a default value, and new indexes. Do not rename or
   drop a table or column, and do not change what an existing column means.
   Any build of the same generation must still be able to read the old columns
   of a newer dump after migrating it.
4. **A build never opens or imports a database that is newer than it.** There
   is no "read the newer schema anyway" mode. Downgrades go through the restore
   flow below.

## When to bump `DB_COMPAT_VERSION`

Bump it in the same PR as the schema change when any of these is true:

- the migration is not additive (a rename, a drop, a type or meaning change, or
  data moved between tables);
- the `db.sql` format changes in a way an older build would import wrongly (for
  example, which tables are skipped or preserved on sync, the header, or a new
  statement or PRAGMA beyond `SQL_IMPORT_ALLOWED_PRAGMAS`);
- mixing snapshots of the old and new shape on one remote would silently lose
  or corrupt data, beyond the "an older upload drops the newer columns" case
  described below.

Do **not** bump it for additive changes. A bump moves the new build to a new
remote folder (`db-v7/`). It no longer sees, and no longer overwrites,
snapshots from older generations. Treat a bump like a breaking change and call
it out in the release notes.

## Mixed-version sync

Sync uploads and downloads a whole snapshot (`db.sql` plus `skills.zip`), and
the last writer wins. For devices on the same `DB_COMPAT_VERSION`:

| The remote snapshot was written by    | What the downloading device does                                                                                                                                                                                                                         |
| ------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| the same or an older `SCHEMA_VERSION` | Imports it and migrates it up.                                                                                                                                                                                                                           |
| a newer `SCHEMA_VERSION`              | Refuses it with the localized `backup.sql.newer_schema` error ("update the app"). The check runs on the `PRAGMA user_version` line before skills or the database are touched, and again on the imported temp database, so nothing is imported partially. |

For devices on different `DB_COMPAT_VERSION` values, `validate_manifest_compat`
rejects the manifest with `sync.manifest_db_version_incompatible`. Normally the
different remote folders already keep the generations apart.

There is a known limit, and it is accepted. If an older device uploads after a
newer one, the remote snapshot no longer has the newer tables or columns. A
newer device that downloads it next loses that data. Upgrade every device that
shares a remote.

Manual SQL import (Settings, Import) follows the same rule. A dump from a newer
`SCHEMA_VERSION` is rejected with `backup.sql.newer_schema`, before the
pre-import backup is taken and before anything is written.

## Downgrade and channel-switch restore flow

1. Before any DDL, `Database::init` snapshots an older database into
   `backups/db_backup_*.db` (the pre-migration backup, CCS-089). After an
   upgrade, that backup keeps the old `user_version`.
2. If an older build later starts against the upgraded database, the startup
   precheck (`stored_user_version_exceeds_supported`) stops before any write.
   It reports `kind: "db_version_too_new"` with `restorable_backups`, the
   backups whose `user_version <= SCHEMA_VERSION`, newest first.
3. The recovery screen (`src/components/DatabaseUpgrade.tsx`) offers "upgrade
   app" and also lists those backups. "Restore" calls
   `restore_backup_over_newer_db`, which works only in this recovery state:
   - It validates the filename, rejects a backup that is also too new, and
     migrates and validates the backup in a temp database.
   - It keeps the newer database as `backups/db_newer_v{N}_<timestamp>.sqlite`.
     That is not a `.db` file, so it is not listed and backup retention never
     prunes it.
   - It writes the backup over `cc-switch.db` and restarts the app.
4. Changes made after the backup was taken are not in the restored database.
   Upgrading the app again and restoring `db_newer_*.sqlite` by hand (renamed to
   `cc-switch.db`) brings them back.

## Checklist for a schema PR

- [ ] `SCHEMA_VERSION` bumped, with a step appended to `MIGRATIONS` and a `tests/fixtures/db/v{N}.sql` fixture
- [ ] The step declares `db_compat`: `Additive`, or `Breaking` with the `DB_COMPAT_VERSION` bump and release notes
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml migrations sync_protocol` passes
