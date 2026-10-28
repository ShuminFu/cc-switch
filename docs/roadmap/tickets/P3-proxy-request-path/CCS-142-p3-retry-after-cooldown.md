# CCS-142: [proxy] Honour Retry-After on 429 and 529 with a per-provider cooldown

**Status:** Open · **Kind:** feature · **Labels:** `feature`, `phase:P3`, `p3-low`, `area:proxy`

> **Ticket key:** `p3-retry-after-cooldown` · **Phase:** P3 · **Priority:** p3-low · **Effort:** S · **Depends on:** `p3-attempt-recorder`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Proxy review idea 3. Rate-limit responses count as breaker failures and are retried immediately.

## Proposed change
- Parse Retry-After (seconds or an HTTP date) and put the provider in a cooldown that select_providers respects.
- Do not count it as a breaker failure.

## Acceptance criteria / verification
- [ ] E2E: P1 returns 429 with Retry-After 2, so requests go to P2 for about 2 s and then back to P1; the breaker stays Closed
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_e2e`

## Dependencies
p3-attempt-recorder
