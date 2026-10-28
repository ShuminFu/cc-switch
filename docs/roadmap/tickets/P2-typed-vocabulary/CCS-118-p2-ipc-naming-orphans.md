# CCS-118: [ipc] Normalize command and argument casing, and wire up or delete orphan commands

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P2`, `p3-low`, `area:ipc`

> **Ticket key:** `p2-ipc-naming-orphans` · **Phase:** P2 · **Priority:** p3-low · **Effort:** M · **Depends on:** `p2-bindings-generate`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Command names and arguments use mixed casing (queryProviderUsage, testUsageScript, 41 non_snake_case args), and 14 commands are never called, including parse_deeplink.

## Proposed change
Rename through the bindings, then delete the orphans or wire them up (list them in the PR).

## Acceptance criteria / verification
- [ ] A `#![deny(non_snake_case)]` check in commands/ passes
- [ ] An orphan check script reports 0
- [ ] `pnpm typecheck`, `pnpm test:unit`, `cargo test --manifest-path src-tauri/Cargo.toml`

## Dependencies
p2-bindings-generate
