# CCS-143: [proxy] Optionally keep each session_id on one provider so prompt caches survive

**Status:** Open · **Kind:** feature · **Labels:** `feature`, `phase:P3`, `p3-low`, `area:proxy`

> **Ticket key:** `p3-session-sticky-routing` · **Phase:** P3 · **Priority:** p3-low · **Effort:** S · **Depends on:** `p3-failover-flapping`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Proxy review idea 4. Routing always starts at the head of the queue, so a session that failed over loses its prompt cache when it returns.

## Proposed change
Add an opt-in setting: a bounded LRU mapping session_id to provider. Prefer the sticky provider while its breaker is available.

## Acceptance criteria / verification
- [ ] E2E: with the setting on, after a failover the same session_id stays on P2 until P2 fails
- [ ] The LRU is bounded
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_e2e`

## Dependencies
p3-failover-flapping
