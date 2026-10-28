# CCS-157: [pricing] Add the pricing catalog, resolver and a single cost engine, and delete the hand-written backfill

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p2-medium`, `area:pricing`

> **Ticket key:** `p4-pricing-engine` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** L · **Depends on:** `p4-migration-framework`, `p1-usage-input-semantics`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Evidence
- usage_stats.rs:2004-2275 resolver; schema.rs:1516-2745 (~880-line seed and repair list); usage_stats.rs:1783-1871 backfill

## Proposed change
- pricing/catalog.json, resolver.rs and engine.rs.
- Backfill and ingest both call engine::cost.
- Remove the seed and repair code from schema.rs.

## Acceptance criteria / verification
- [ ] One cost function (grep check)
- [ ] The cost golden table has at least 50 rows and passes; backfill output equals ingest output for the same row
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml pricing cost_golden`

## Dependencies
p4-migration-framework, p1-usage-input-semantics
