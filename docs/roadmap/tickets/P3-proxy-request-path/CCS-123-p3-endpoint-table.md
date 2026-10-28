# CCS-123: [proxy] Build routes from a ClientEndpoint table with one handle_proxied and per-format error renderers

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P3`, `p2-medium`, `area:proxy`

> **Ticket key:** `p3-endpoint-table` · **Phase:** P3 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p2-proxy-format-types`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Five near-duplicate handlers (about 600 lines) and a copied compact Responses handler.

## Evidence
- proxy/handlers.rs:156, 692, 779, 894, 1723; compact copy at 894-985

## Proposed change
- Add proxy/endpoints.rs with the ClientEndpoint table; server.rs generates its routes from it.
- A single handle_proxied(&ClientEndpoint).
- Add proxy/errors/render.rs with one renderer per client WireFormat.

## Acceptance criteria / verification
- [ ] Golden fixtures are byte-identical; proxy_e2e is green
- [ ] handlers.rs line count shrinks by at least 500
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_e2e --test proxy_golden`

## Dependencies
p2-proxy-format-types
