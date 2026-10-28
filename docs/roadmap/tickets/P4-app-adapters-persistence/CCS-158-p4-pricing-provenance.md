# CCS-158: [pricing] Add pricing provenance and tombstones, classifying existing edited rows as user-owned

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p2-medium`, `area:pricing`

> **Ticket key:** `p4-pricing-provenance` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p4-pricing-engine`, `p1-pricing-no-reseed`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
The tombstone was moved here from P1 (critique). Existing rows do not record whether the user edited them, so a catalog swap could overwrite user prices or bring back deleted ones.

## Proposed change
- Migration: add source, catalog_version, updated_at and deleted columns.
- Classify every existing row that differs from the seed as source=user.
- Migrate the P1 settings-stored deletions into tombstones.

## Acceptance criteria / verification
- [ ] Tests: an edited row survives a catalog update; a deleted built-in stays deleted across a catalog update and restart
- [ ] Migration harness green
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml pricing --test migrations`

## Dependencies
p4-pricing-engine, p1-pricing-no-reseed
