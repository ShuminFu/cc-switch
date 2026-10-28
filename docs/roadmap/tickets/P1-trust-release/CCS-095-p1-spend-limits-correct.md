# CCS-095: [usage] Fix spend-limit windows, add an index and surface DB errors

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p2-medium`, `area:usage`

> **Ticket key:** `p1-spend-limits-correct` · **Phase:** P1 · **Priority:** p2-medium · **Effort:** S · **Depends on:** `p1-db-compat-policy`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
check_provider_limits computes its day/month windows in SQL and returns $0 when a DB error occurs. The critique notes this is a UI-only check (it is never enforced in the proxy; see p4-spend-cap-enforcement).

## Evidence
- src-tauri/src/services/usage_stats.rs:1599-1680 (:1648, :1667 unwrap_or(0.0)); commands/usage.rs:257

## Proposed change
- Compute [start,end) in Rust with compute_local_midnight_cutoff.
- Add `CREATE INDEX IF NOT EXISTS` on proxy_request_logs(provider_id, app_type, created_at). It is additive, with no version bump, per the compatibility policy.
- Propagate errors.

## Acceptance criteria / verification
- [ ] EXPLAIN QUERY PLAN shows an index seek
- [ ] Test: a DB error produces an Err, not 0.0
- [ ] Test: a boundary row at midnight lands in the correct day
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml usage_stats`

## Dependencies
p1-db-compat-policy
