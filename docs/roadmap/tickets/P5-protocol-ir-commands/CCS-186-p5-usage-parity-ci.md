# CCS-186: [usage] Run a usage parity checker over the fixture corpus in CI

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P5`, `p3-low`, `area:usage-tests`

> **Ticket key:** `p5-usage-parity-ci` · **Phase:** P5 · **Priority:** p3-low · **Effort:** S · **Depends on:** `p3-usage-parity-badge`, `p5-codec-responses`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
A test that, for every golden fixture, compares the UsageIR-normalized totals with the raw upstream usage and the per-semantics cost table.

## Acceptance criteria / verification
- [ ] 0 mismatches; CI fails on any mismatch
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml usage_parity`

## Dependencies
p3-usage-parity-badge, p5-codec-responses
