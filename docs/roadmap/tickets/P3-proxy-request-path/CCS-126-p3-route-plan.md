# CCS-126: [proxy] Resolve a RoutePlan once per attempt and carry it in ForwardResult

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P3`, `p1-high`, `area:proxy`

> **Ticket key:** `p3-route-plan` · **Phase:** P3 · **Priority:** p1-high · **Effort:** M · **Depends on:** `p3-endpoint-table`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
The request format and the response decoding are derived separately, so they can disagree after failover.

## Evidence
- forwarder.rs:2432 resolve_claude_api_format, 1129-1412 booleans; handlers.rs:229, 837-858, 954-975 re-derive

## Proposed change
- Add proxy/routing/plan.rs: RoutePlan {client, upstream, url, AuthStrategy, VendorQuirks, side stores}.
- ForwardResult carries the plan.
- Log the plan for each attempt.

## Acceptance criteria / verification
- [ ] New E2E: failover from a Copilot (openai_responses) provider to an Anthropic provider decodes with the plan's format
- [ ] Golden fixtures are identical
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_e2e`

## Dependencies
p3-endpoint-table
