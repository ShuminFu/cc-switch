# CCS-166: [apps] Detect live-config drift and external edits, and offer reconcile

**Status:** Open · **Kind:** feature · **Labels:** `feature`, `phase:P4`, `p2-medium`, `area:apps`

> **Ticket key:** `p4-drift-detection` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p4-takeover-engine`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Merges the plan backlog item with backend idea 4.
- Leftover takeover placeholders after a crash, and external edits to hooks or permissions, are lost on the next switch.

## Proposed change
- Watch the live files (notify crate) and compare them with the expected adapter output.
- On drift, offer: reconcile (repair placeholders), fold external fields into the common config, or ignore.

## Acceptance criteria / verification
- [ ] Tests: a leftover PROXY placeholder after a simulated crash is detected and repaired; an externally added `hooks` key is offered for folding
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml drift` and `pnpm test:unit`

## Dependencies
p4-takeover-engine
