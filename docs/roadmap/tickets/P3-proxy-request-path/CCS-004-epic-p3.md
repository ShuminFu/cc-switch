# CCS-004: [epic] P3 (v3.21): Proxy request path with endpoint table, RoutePlan, attempt pipeline, canonical usage and stream taps

**Status:** Open · **Kind:** epic · **Labels:** `epic`, `phase:P3`, `p1-high`, `area:proxy`

> **Ticket key:** `epic-p3` · **Phase:** P3 · **Priority:** p1-high · **Effort:** XL · **Depends on:** `epic-p2`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Goal
Make one decision per attempt that drives both request encoding and response decoding. Break up forward_with_retry_inner and forward(). Write the cache-token arithmetic and the stop-reason mapping once each. Ship bug-report capture (moved here from P2) on the new taps. All golden fixtures stay identical.

## Exit criteria
- [ ] Every golden fixture, including upstream_request.expected, is byte-identical after normalization, or the diff is a documented fix; proxy_e2e is green on 3 OSes
- [ ] forwarder.rs is 600 lines or fewer; no function in pipeline/ exceeds 150 lines; handlers.rs is 1200 lines or fewer; forward() has exactly one call site
- [ ] Failover from a Copilot (openai_responses) provider to an Anthropic provider decodes with the plan's format (E2E)
- [ ] Zero DB reads on the hot path (proxy_e2e counter); p50 overhead is no worse than the P0 baseline; UsageTap memory does not grow with the event count
- [ ] Cache-token arithmetic lives only in UsageIR::from_*; stream and non-stream paths return the same stop_reason for each bridge
- [ ] Exported captures contain no keys (redaction corpus test)

## Child tickets
p3-endpoint-table, p3-runtime-config-snapshot, p3-router-lock-db-read, p3-route-plan, p3-attempt-recorder, p3-failover-flapping, p3-recovery-strategy, p3-request-pipeline-stages, p3-header-hop-by-hop, p3-header-case-per-request, p3-breaker-sliding-window, p3-usage-ir, p3-stop-reason-table, p3-sse-decoder, p3-stream-traits-taps, p3-bug-report-capture, p3-trace-inspector, p3-usage-parity-badge, p3-breaker-ui-probe, p3-retry-after-cooldown, p3-session-sticky-routing, p3-deps-dedupe-http
