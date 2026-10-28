# CCS-159: [db] Add usage and session-cursor repositories with a UsageQuery builder and transactional ingest

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p2-medium`, `area:database`

> **Ticket key:** `p4-usage-repo` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** L · **Depends on:** `p4-migration-framework`, `p1-session-sync-final-entry`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
- Add database/repo/{usage,pricing,session_cursor}.rs, a UsageQuery builder with keyset pagination, and a UsageSource enum.
- Session ingest writes one transaction per file together with its cursor.
- Add a CI grep: no SQL outside database/.

## Acceptance criteria / verification
- [ ] Test: kill the process between files during import, re-run, and there is no double counting
- [ ] The CI grep for `SELECT |INSERT |UPDATE ` outside src-tauri/src/database passes
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml repo::`

## Dependencies
p4-migration-framework, p1-session-sync-final-entry
