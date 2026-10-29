# CCS-090: [db] Enable WAL, synchronous=NORMAL and busy_timeout on local DB paths

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P1`, `p2-medium`, `area:database`

> **Ticket key:** `p1-db-wal-busy-timeout` · **Phase:** P1 · **Priority:** p2-medium · **Effort:** S · **Depends on:** `p1-db-premigration-backup`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
The DB runs in DELETE journal mode with no busy_timeout, so locks contend between the proxy logger, session sync and UI queries.

## Evidence
- src-tauri/src/database/mod.rs Database::init; set_app_config_dir_override (settings.rs:296) can point at iCloud or Dropbox

## Proposed change
- Verify snapshot_to_memory, WebDAV/S3 export and update_hook under WAL.
- Set `journal_mode=WAL`, `synchronous=NORMAL` and `busy_timeout=5000`.
- Specify non-local detection: the override dir is inside a known cloud-sync root (iCloud Drive, Dropbox, OneDrive, Google Drive), or a statfs network filesystem type. In that case keep DELETE mode and log it.

## Acceptance criteria / verification
- [x] A startup log/test asserts `PRAGMA journal_mode` == wal on a local temp dir and delete on a simulated cloud path
- [x] Existing sync/export tests pass under WAL
- [x] `cargo test --manifest-path src-tauri/Cargo.toml database::`

## Dependencies
p1-db-premigration-backup

## Implementation notes

**Summary.** `Database::init` now calls `database::journal::configure_file_connection` right after
`foreign_keys` / `auto_vacuum` (WAL must come after `auto_vacuum`, because switching to WAL writes the
header of a brand-new DB). It always sets `busy_timeout=5000`. On local storage it sets
`journal_mode=WAL` + `synchronous=NORMAL`. When the data directory is non-local it explicitly sets
`journal_mode=DELETE`, so a DB that was WAL before and then moved returns to DELETE, keeps the default
`synchronous=FULL`, and logs `Database directory … is on …; keeping journal_mode=DELETE`. A failed
journal switch, for example because another process holds the DB, only logs a warning and does not
block startup.

Non-local detection (`non_local_storage_reason`):
- Cloud-sync roots: any path component matched case-insensitively. These are `Mobile Documents`,
  `com~apple~CloudDocs`, `iCloud Drive` and `iCloudDrive`; `Library/CloudStorage` (the macOS File
  Provider); `Dropbox` and `Dropbox (…)`; `OneDrive`, `OneDrive - …` and `OneDrive-…`; and
  `Google Drive`, `GoogleDrive`, `GoogleDrive-…` and `My Drive`.
- Network filesystems: on Linux, `statfs` `f_type` matching nfs, smb, cifs, smb2, ncp, coda, afs, 9p,
  ceph, lustre or gpfs. On macOS, `statfs` without `MNT_LOCAL`. On Windows, UNC / `\\?\UNC` prefixes
  or `GetDriveTypeW == DRIVE_REMOTE`. FUSE is deliberately not treated as network.

Under WAL I checked the following:
- `snapshot_to_memory` and the WebDAV/S3 export, which goes through `export_sql_string_for_sync`, use
  the Online Backup API and see committed WAL data.
- `update_hook` is per-connection and does not change.
- Restore copies into the WAL main DB, and the main DB stays in WAL mode.
- The Backup API copies the WAL flag into snapshot backup files, and opening one read-only would leave
  `-wal`/`-shm` files in `backups/`. `snapshot_connection_to_backup` therefore switches each backup
  file to DELETE.

**Files:** `src-tauri/src/database/journal.rs` (new), `src-tauri/src/database/mod.rs`,
`src-tauri/src/database/backup.rs`, `src-tauri/src/database/tests.rs`, `src-tauri/Cargo.toml`
(`libc = "0.2"` for macOS/Linux, already in the lockfile), `src-tauri/Cargo.lock`.

**Commits:** d5d8ccf (code + tests); this ticket update follows in a separate docs commit.

**Tests added:**
- `database::tests::init_uses_wal_with_busy_timeout_on_local_dir`: `Database::init` on a temp home
  gives journal_mode=wal, synchronous=1, busy_timeout=5000 and auto_vacuum=2. A second connection can
  read while a write transaction is open. Backup files come out in delete mode.
  `snapshot_to_memory` and `export_sql_string_for_sync` both see the committed rows.
- `database::tests::init_keeps_delete_journal_in_cloud_sync_dir`: with HOME at `<tmp>/Dropbox`,
  journal_mode=delete, busy_timeout=5000, and no `-wal`/`-shm` files.
- `database::journal::tests::*`: cloud-root matching for positive and negative paths, temp dir
  counts as local, and a WAL DB moved into a cloud dir is switched back to DELETE.

Both `init_*` tests fail without this change, because the journal mode stayed `delete` and
busy_timeout was 0.

**Verification:**
- `cargo fmt --check`: clean.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: clean.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib database::`: 57 passed, 0 failed.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: lib 2247 passed / 0 failed /
  3 ignored. All integration suites pass, including `migrations` (WAL pre-migration backup) and
  `import_export_sync`.
- The macOS and Windows `network_filesystem` branches can't be built or run here. I extracted them
  into a scratch crate with the same libc 0.2.183 and windows-sys 0.61.2 and ran
  `cargo clippy -D warnings` for `x86_64-apple-darwin` and `x86_64-pc-windows-gnu`: clean. They have
  not been run on real macOS or Windows hosts, or on real network or cloud mounts.
