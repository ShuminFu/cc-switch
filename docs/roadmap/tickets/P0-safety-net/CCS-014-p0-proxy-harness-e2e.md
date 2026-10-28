# CCS-014: [proxy-tests] Add ProxyHarness with a mock upstream and an end-to-end proxy suite with baselines

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p1-high`, `area:proxy-tests`

> **Ticket key:** `p0-proxy-harness-e2e` · **Phase:** P0 · **Priority:** p1-high · **Effort:** L · **Depends on:** `p0-testenv-paths`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
No test drives the real router, forwarder and logger end to end. Failover, breaker transitions and usage logging are covered only in pieces.

## Evidence
- proxy/server.rs:291-369 build_router; forwarder.rs forward_with_retry_inner; response_processor.rs usage logging

## Proposed change
- Add `ProxyHarness::spawn(MockUpstream)`, an axum mock on 127.0.0.1:0.
- Add tests/proxy_e2e.rs with at least 8 scenarios: two-provider failover, breaker open, half-open, each rectifier, SSE passthrough, and proxy_request_logs rows.
- Add a DB-query counter, plus latency and allocation baselines saved as a JSON artifact.

## Acceptance criteria / verification
- [ ] At least 8 scenarios, green on ubuntu, macos and windows in CI
- [ ] Baseline numbers committed to docs/dev/perf-baseline.json
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_e2e`

## Dependencies
p0-testenv-paths
