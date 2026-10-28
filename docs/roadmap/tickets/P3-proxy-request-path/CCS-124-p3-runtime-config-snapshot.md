# CCS-124: [proxy] Cache runtime config in an ArcSwap snapshot and remove DB reads from the request path

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P3`, `p2-medium`, `area:proxy`

> **Ticket key:** `p3-runtime-config-snapshot` · **Phase:** P3 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p3-endpoint-table`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Each request does about 5 DB or config reads.

## Evidence
- proxy/handler_context.rs:95-110; provider_router.rs:43/134; the RequestForwarder 17-argument constructor

## Proposed change
- Add `ProxyState.runtime_config: ArcSwap<RuntimeConfigSnapshot>`, refreshed in apply_runtime_config and update_config.
- Replace the constructor with `RequestForwarder { shared: Arc<ForwarderDeps>, req: RequestScope }`.

## Acceptance criteria / verification
- [ ] The proxy_e2e DB-query counter reads 0 per request
- [ ] p50 latency is no worse than the P0 baseline
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_e2e`

## Dependencies
p3-endpoint-table
