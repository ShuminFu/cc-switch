# CCS-145: [db] Add a versioned migration framework with a frozen baseline and audit table

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p1-high`, `area:database`

> **Ticket key:** `p4-migration-framework` · **Phase:** P4 · **Priority:** p1-high · **Effort:** M · **Depends on:** `p0-migration-harness`, `p1-db-compat-policy`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Migrations live in a user_version loop, 8 ALTERs swallow their errors, and create_tables does repairs. Per the critique this must land before any schema change, including the tombstone that was moved out of P1.

## Evidence
- src-tauri/src/database/schema.rs (~:430 loop, create_tables repairs)

## Proposed change
- Add database/migrations/ with an ordered MIGRATIONS table, a frozen V1 baseline and a schema_migrations audit table.
- Turn the swallowed ALTERs into numbered steps that propagate errors, and delete create_tables.
- Require each migration to declare a DB_COMPAT_VERSION impact, following p1-db-compat-policy.

## Acceptance criteria / verification
- [ ] A CI test asserts MIGRATIONS is contiguous and ends at SCHEMA_VERSION
- [ ] The migration harness passes from every vN and from JSON; fresh and migrated DBs are equivalent
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test migrations`

## Dependencies
p0-migration-harness, p1-db-compat-policy
