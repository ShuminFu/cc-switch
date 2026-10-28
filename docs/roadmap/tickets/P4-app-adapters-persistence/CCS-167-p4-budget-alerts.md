# CCS-167: [usage] Add budget alerts on usage-log-recorded

**Status:** Open · **Kind:** feature · **Labels:** `feature`, `phase:P4`, `p3-low`, `area:usage`

> **Ticket key:** `p4-budget-alerts` · **Phase:** P4 · **Priority:** p3-low · **Effort:** S · **Depends on:** `p4-usage-repo`, `p1-spend-limits-correct`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Limits are pull-only today.

## Proposed change
On each usage-log-recorded event, evaluate the provider's daily and monthly limits (from p1-spend-limits-correct) and send a system notification at 80% and 100%, deduped per period.

## Acceptance criteria / verification
- [ ] Test: crossing 80% emits one alert per period
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml budget` and `pnpm test:unit`

## Dependencies
p4-usage-repo, p1-spend-limits-correct
