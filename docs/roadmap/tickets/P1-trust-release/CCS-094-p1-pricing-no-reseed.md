# CCS-094: [pricing] Stop reseeding built-in prices on every read so deleted prices stay deleted

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p2-medium`, `area:pricing`

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
- [ ] Integration test: delete a built-in price, restart, list prices; the price stays deleted
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml pricing`

## Dependencies
None.
