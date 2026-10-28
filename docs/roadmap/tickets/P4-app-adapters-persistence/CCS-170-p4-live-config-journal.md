# CCS-170: [apps] Keep a live-config journal with one-click undo of the last switch

**Status:** Open · **Kind:** feature · **Labels:** `feature`, `phase:P4`, `p3-low`, `area:apps`

> **Ticket key:** `p4-live-config-journal` · **Phase:** P4 · **Priority:** p3-low · **Effort:** M · **Depends on:** `p4-adapter-additive`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Backend idea 3 and frontend idea 7. A bad switch cannot be undone.

## Proposed change
- Keep the last N LiveSnapshot entries per app (0600, under app_config_dir/journal).
- Add a revert_last_switch command and an Undo action on the switch toast.

## Acceptance criteria / verification
- [ ] Test: switch A to B, revert, and the live files are byte-identical to A
- [ ] The journal is bounded
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml journal` and `pnpm test:unit`

## Dependencies
p4-adapter-additive
