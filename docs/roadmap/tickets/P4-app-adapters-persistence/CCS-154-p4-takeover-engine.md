# CCS-154: [proxy] Unify takeover behind takeover(app, Strict|BestEffort) with TakeoverError and a registry-driven status

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p2-medium`, `area:proxy`

> **Ticket key:** `p4-takeover-engine` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** L · **Depends on:** `p4-services-proxy-split`, `p4-adapter-additive`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
There are 3 takeover variants x 4 apps, and the base URL is built in 5 places. The critique notes ProxyTakeoverStatus has no hermes field at all; only opencode and openclaw are hard-coded false.

## Evidence
- services/proxy.rs:1528-1712, 714-716; proxy/types.rs:112-119

## Proposed change
- Use a ProxyTakeover trait on the adapters.
- Turn backup, restore and cleanup into loops.
- Take the base URL from descriptor.proxy_route.
- Add a TakeoverError enum.
- Generate ProxyTakeoverStatus from the registry as a map, keeping the existing serde field names for compatibility.

## Acceptance criteria / verification
- [ ] Crash/restore E2E is green
- [ ] The status JSON is backward compatible (snapshot)
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml takeover` and `pnpm typecheck`

## Dependencies
p4-services-proxy-split, p4-adapter-additive
