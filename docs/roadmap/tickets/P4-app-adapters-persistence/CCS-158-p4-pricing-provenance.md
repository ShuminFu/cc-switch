# CCS-158: [pricing] Add pricing provenance and tombstones, classifying existing edited rows as user-owned

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p2-medium`, `area:pricing`

> **Ticket key:** `p4-pricing-provenance` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p4-pricing-engine`, `p1-pricing-no-reseed`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
The tombstone was moved here from P1 (critique). Existing rows do not record whether the user edited them, so a catalog swap could overwrite user prices or bring back deleted ones.

## Proposed change
- Migration: add source, catalog_version, updated_at and deleted columns.
- Classify every existing row that differs from the seed as source=user.
- Migrate the P1 settings-stored deletions into tombstones.

## Acceptance criteria / verification
- [x] Tests: an edited row survives a catalog update; a deleted built-in stays deleted across a catalog update and restart
- [x] Migration harness green
- [x] `cargo test --manifest-path src-tauri/Cargo.toml pricing --test migrations`

## Dependencies
p4-pricing-engine, p1-pricing-no-reseed

## Implementation notes

**Summary**
- **Schema v18** (`database/migrations/v018.rs`, `DbCompatImpact::Additive`, `DB_COMPAT_VERSION` unchanged): `model_pricing` gains `source TEXT NOT NULL DEFAULT 'user'`, `catalog_version INTEGER`, `updated_at INTEGER` and `deleted INTEGER NOT NULL DEFAULT 0`. The step then runs `pricing::catalog::classify_provenance`. A row whose four prices equal the current catalog price is marked `builtin` with `catalog_version = <catalog version>`. A row equal to a `supersedes[]` price is marked `builtin` with a NULL version. Every other row (edited built-ins, custom models) keeps the default `user`. A row at the current price must also have the current display name, otherwise it is `user` (see Review). Rows at a superseded price are matched on price only, because the catalog keeps no history of names.
- **Catalog sync** (`pricing::catalog::sync` / `sync_with`), run on every start and restore:
  1. It folds the P1 settings list `deleted_builtin_model_pricing` into tombstones (`deleted = 1`, `source = 'user'`), but only for built-ins that have no row. This matches the P1 behaviour when a synced snapshot still has the row.
  2. It runs `INSERT OR IGNORE` for new models as `builtin`.
  3. It moves only `source = 'builtin' AND deleted = 0` rows to the current catalog price, name and version.

  User rows and tombstones are never touched. Provenance now covers what `apply_supersedes` used to do, so that function is gone; `supersedes[]` is still used to classify rows in v18.
- **Writes:** `Database::delete_model_pricing` writes a tombstone for a built-in (an upsert keeps the row with `deleted = 1`) and still dual-writes the settings marker for older builds and v17 snapshots. A custom row is still hard-deleted. The new `Database::save_user_model_pricing` (used by `update_model_pricing`) writes `source = 'user'`, `deleted = 0`, `updated_at = now` and replaces any tombstone, so re-adding a deleted built-in works.
- **Reads:** `get_model_pricing` and both resolver queries filter on `deleted = 0`.
- **Frozen steps:** the v2 and v9 legacy steps call `catalog::legacy_seed`, which is the old base-column seed, so their behaviour does not change.
- **Behaviour change:** clearing only the settings marker no longer resurrects a deleted built-in, because the tombstone lives in the DB. Re-adding the price (`save_user_model_pricing`) is the way to bring it back.
- **Mixed-version sync:** an older device's v17 upload drops the new columns. On import the v18 step re-classifies the rows, and the settings marker, which is still dual-written, rebuilds the tombstones.

**Files**
- `src-tauri/src/database/migrations/v018.rs` (new), `migrations/mod.rs`, `migrations/legacy.rs`, `migrations/tests.rs`
- `src-tauri/src/database/mod.rs` (`SCHEMA_VERSION = 18`), `database/schema.rs`, `database/tests.rs`
- `src-tauri/src/pricing/catalog.rs`, `pricing/resolver.rs`
- `src-tauri/src/commands/usage.rs`, `src-tauri/src/settings.rs` (doc comment)
- `src-tauri/tests/migrations.rs`, `src-tauri/tests/fixtures/db/v18.sql` (new)

**Fixture:** I generated `v18.sql` with `CCS_FIXTURE_LOCKFILE=current scripts/gen-db-fixture.sh` from a temporary commit (`f5433bd`). Its `src-tauri/src` differs from `b4631b2` only by a doc comment in `settings.rs`, and the header points at `b4631b2`.

**Commits:** `b4631b2` (code, tests, fixture) and a follow-up commit for this note and the fixture header.

**Verification**
- New tests:
  - `pricing::catalog::tests::pricing_catalog_update_keeps_user_rows_and_tombstones`: a simulated catalog v+1 reprices an edited row, a deleted built-in and an untouched row. Only the untouched one changes, and the tombstone does not resolve.
  - `pricing::catalog::tests::pricing_legacy_settings_deletions_become_tombstones`
  - `database::migrations::tests::migrate_v17_to_v18_classifies_pricing_provenance`
  - `database::tests::deleted_builtin_model_pricing_stays_deleted_after_restart`, extended: the row stays deleted across a catalog update and restarts, and a re-add is user-owned.
  - `tests/migrations.rs::pricing_provenance_survives_upgrade_from_v17`: a v17 fixture with a P1 settings deletion, upgraded and then restarted.
- `cargo test --manifest-path src-tauri/Cargo.toml pricing --test migrations`: 1 passed.
- `cargo test --test migrations`: 14 passed. This covers all 17 DB fixtures v1–v13 and v15–v18, plus v0 and the legacy JSON fixtures.
- `cargo test --lib pricing` (31 passed) and `cargo test --lib migrat` (56 passed).
- The full `cargo test --no-fail-fast`: 2585 passed, 0 failed.
- `cargo fmt --check` is clean, and `cargo clippy --all-targets -- -D warnings` is clean.

**Review**
- Found one defect. The v18 classification compared prices only, so a built-in that the user had only renamed (current price, different display name) became `builtin`. The startup sync that runs right after the migration then set its name back to the catalog name, which lost a user edit. Before v18, `apply_supersedes` never touched such a row. The fix is in `classify_provenance`: a current-price row is `builtin` only when its display name also equals the catalog name. Superseded-price rows are still matched on price only, so they are upgraded and renamed on start, as before v18. I checked the available git history of the old seed list and found no built-in that was published with the same price under a different name, so the stricter check does not freeze untouched rows.
- Tests: `migrate_v17_to_v18_classifies_pricing_provenance` now expects the renamed-only row to be `user`. `tests/migrations.rs::pricing_provenance_survives_upgrade_from_v17` now asserts that a renamed built-in keeps its name across the upgrade and a restart.
- Everything else checked out: all acceptance criteria, every `model_pricing` reader (the list, the exact and prefix resolver queries, and backfill through the resolver), the delete and re-add paths, and backup restore, which runs migrations and then sync.
- Verification: `cargo test --test migrations` 14 passed. `cargo test --lib` pricing/migration filters 85 passed. Full `cargo test --no-fail-fast` 2585 passed, 0 failed. `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` are clean.
