# CCS-050: [proxy] Enforce the request body limit and return 400 for malformed JSON

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p3-low`, `area:proxy`

> **Ticket key:** `p1-proxy-body-limit` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding proxy-body-limit-bypass. Handlers take axum::extract::Request and call body.collect(), which skips DefaultBodyLimit, so the body size is unlimited. Parse and read errors map to 500.

## Evidence
- src-tauri/src/proxy/handlers.rs:169-175, 702-708, 792-798, 907-913, 1733-1743; server.rs:368; error.rs:150/156

## Proposed change
Add one helper using axum::body::to_bytes(body, LIMIT): it returns 413 when the limit is exceeded and maps serde errors to ProxyError::InvalidRequest (400). Use it at all 5 sites.

## Acceptance criteria / verification
- [ ] Router test: `{not json` returns 400
- [ ] A body over a small test limit returns 413
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml proxy::`

## Dependencies
None.
