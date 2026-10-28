# CCS-161: [db] Store rollup cost as integer micro-USD and add RetentionPolicy with Maintenance::run

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p3-low`, `area:database`

> **Ticket key:** `p4-rollup-retention` · **Phase:** P4 · **Priority:** p3-low · **Effort:** M · **Depends on:** `p4-usage-repo`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Evidence
- database/mod.rs:148 and backup.rs:271 hard-coded retention literals; floating-point rollups

## Proposed change
- Migration to micro-USD integers for rollups.
- RetentionPolicy {detail_days, rollup_days, backups_keep} in settings, with Maintenance::run on a schedule.

## Acceptance criteria / verification
- [ ] Rollup totals equal the sum of the detail rows exactly (test)
- [ ] Retention prunes per the policy (test)
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml maintenance`

## Dependencies
p4-usage-repo
