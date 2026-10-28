# CCS-168: [proxy] Enforce per-provider spend and token caps in provider selection

**Status:** Open · **Kind:** feature · **Labels:** `feature`, `phase:P4`, `p2-medium`, `area:proxy`

> **Ticket key:** `p4-spend-cap-enforcement` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p4-usage-repo`, `p3-runtime-config-snapshot`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
The critique notes limits are not enforced at all; check_provider_limits is called only from the UI (commands/usage.rs:257). Proxy review idea 6.

## Proposed change
- Keep running per-provider totals in memory, seeded from the UsageRepo.
- select_providers skips a provider over its cap and fails over. With no candidate left, return a clear 429-style error rendered per client format.

## Acceptance criteria / verification
- [ ] E2E: P1 at its cap routes to P2; with all at cap, the error renders in Anthropic format
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_e2e`

## Dependencies
p4-usage-repo, p3-runtime-config-snapshot
