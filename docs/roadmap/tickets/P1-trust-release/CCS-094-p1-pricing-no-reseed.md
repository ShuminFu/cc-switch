# CCS-094: [pricing] Stop reseeding built-in prices on every read so deleted prices stay deleted

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p2-medium`, `area:pricing`

> **Ticket key:** `p1-pricing-no-reseed` · **Phase:** P1 · **Priority:** p2-medium · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Reading model pricing reseeds the built-ins, which brings deleted rows back. Per the critique, the tombstone column needs a schema migration and therefore moves to P4 (p4-pricing-provenance). This ticket stops the per-read reseed without changing the schema.

## Evidence
- src-tauri/src/commands/usage.rs:127 (ensure_model_pricing_seeded on read)

## Proposed change
- Seed only at startup when the pricing table is empty or the seed version marker changes.
- Store the deleted built-in model ids in the settings local_migrations area (no DDL).

## Acceptance criteria / verification
- [x] Integration test: delete a built-in price, restart, list prices; the price stays deleted
- [x] `cargo test --manifest-path src-tauri/Cargo.toml pricing`

## Dependencies
None.

## Implementation notes

**Summary.** `get_model_pricing` no longer calls `ensure_model_pricing_seeded`; built-in prices are seeded only at startup (`Database::init` / `Database::memory`) and when restoring a backup. Deleting a built-in price (new `Database::delete_model_pricing`, used by the `delete_model_pricing` command) first records the model id in `settings.json` → `localMigrations.deletedBuiltinModelPricing` (no schema change), and `seed_model_pricing` skips those ids, so the row stays deleted across restarts. Saving the same model id again through `update_model_pricing` clears the marker. Deleting a custom (non-built-in) price records nothing. The marker is preserved across frontend settings saves like the other `local_migrations` entries.

Deviation from the proposed change: no separate "seed version marker" was added. The startup seed is still `INSERT OR IGNORE` (so new built-in models still show up after an upgrade), and the tombstone list is what keeps deleted rows from coming back, so a version marker was not needed. Rows are still not tombstoned in the DB itself; that is P4 (`p4-pricing-provenance`).

**Files**
- `src-tauri/src/commands/usage.rs`: drop the reseed from the read path; delete goes through `Database::delete_model_pricing`; update clears the deleted marker
- `src-tauri/src/database/schema.rs`: built-in list moved to `builtin_model_pricing()`, plus `is_builtin_model_pricing`, a tombstone-aware `seed_model_pricing` and `Database::delete_model_pricing`
- `src-tauri/src/settings.rs`: `LocalMigrations::deleted_builtin_model_pricing`, `deleted_builtin_model_pricing_ids()`, `set_builtin_model_pricing_deleted()`
- `src-tauri/src/database/tests.rs`: regression test `deleted_builtin_model_pricing_stays_deleted_after_restart`
- `src-tauri/src/commands/settings.rs`: the settings-merge test also checks that the new marker is preserved

**Commits:** `55bc659` (code + tests); this ticket update is in a follow-up commit.

**Verification**
- `cargo test --manifest-path src-tauri/Cargo.toml --lib pricing`: 19 passed, 0 failed
- Regression check: with the tombstone filter disabled temporarily, `deleted_builtin_model_pricing_stays_deleted_after_restart` fails ("deleted builtin pricing was reseeded on restart"); with the fix it passes
- `cargo fmt --check`: clean; `cargo clippy -- -D warnings`: clean
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast` (lib + integration + doc): 2429 passed, 0 failed
