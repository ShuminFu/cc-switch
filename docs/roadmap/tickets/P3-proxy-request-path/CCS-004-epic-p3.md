# CCS-004: [epic] P3 (v3.21): Proxy request path with endpoint table, RoutePlan, attempt pipeline, canonical usage and stream taps

**Status:** Partial · **Kind:** epic · **Labels:** `epic`, `phase:P3`, `p1-high`, `area:proxy`

> **Ticket key:** `epic-p3` · **Phase:** P3 · **Priority:** p1-high · **Effort:** XL · **Depends on:** `epic-p2`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Goal

Make one decision per attempt that drives both request encoding and response decoding. Break up forward_with_retry_inner and forward(). Write the cache-token arithmetic and the stop-reason mapping once each. Ship bug-report capture (moved here from P2) on the new taps. All golden fixtures stay identical.

## Exit criteria

- [ ] Every golden fixture, including upstream_request.expected, is byte-identical after normalization, or the diff is a documented fix; proxy_e2e is green on 3 OSes — _Audit 2026-10-30: fixture half met (only documented changes: CCS-135 renamed the Gemini drift fixture to a parity fixture with an empty diff; CCS-186/187 only add fixtures; `proxy_golden` 18/18 and `proxy_e2e` 37/37 pass on Linux). Unmet: no recorded green run on macOS/Windows. `ci.yml` `backend` runs `cargo test` on ubuntu/windows/macos, but GitHub has no ci.yml run on the default branch yet._
- [ ] forwarder.rs is 600 lines or fewer; no function in pipeline/ exceeds 150 lines; handlers.rs is 1200 lines or fewer; forward() has exactly one call site — _Audit: forwarder.rs is **627** lines (CCS-130 left it at 556; CCS-139 took it to 599, CCS-142 to 616, CCS-143 to 625, CCS-187 to 627), so this is unmet. The other parts are met: the longest function in pipeline/ is 113 lines (`validate_responses_stream_start`); handlers.rs is 1050; `.forward(` has one production call site (forwarder.rs:394), plus a test shim in forwarder/tests.rs. `forward()` itself, still in forwarder.rs, is 158 lines._
- [x] Failover from a Copilot (openai_responses) provider to an Anthropic provider decodes with the plan's format (E2E) — _`proxy_e2e::failover_from_copilot_responses_to_anthropic_decodes_with_the_anthropic_plan` passes, along with the two reverse/Responses failover tests (CCS-126). The harness has no Copilot token source, so the Copilot attempt fails before it sends anything; the decode format still comes from the plan._
- [ ] Zero DB reads on the hot path (proxy_e2e counter); p50 overhead is no worse than the P0 baseline; UsageTap memory does not grow with the event count — _Audit: zero DB reads is met (`perf_baseline_non_streaming_request` gives `db_reads_per_request: 0.0` and enforces it as a gate). The UsageTap part is met (`stream::taps::tests::usage_tap_memory_is_constant_in_event_count` uses 100k events and passes). p50 is not confirmed for the current code: 5 debug runs today gave 3.11–3.50 ms on a shared host at load ~1.9, against 2.87 ms for the P0 baseline (fc90858) and 2.28 ms recorded after CCS-124. allocations_per_request rose from 582 to 815 after CCS-124, which is still inside the 3x gate._
- [x] Cache-token arithmetic lives only in UsageIR::from**; stream and non-stream paths return the same stop_reason for each bridge — *The only cache subtraction outside tests is in `usage/ir.rs` (`from_total_input`, which `from*_`uses). The other matches are comments and test pins. Every bridge has a`parity\__`fixture (CCS-135), and no fixture pins a stop_reason diff. The single remaining non-empty`expected_diff` is a content-block layout diff.\*
- [x] Exported captures contain no keys (redaction corpus test) — _`proxy::capture::tests::capture_export_has_zero_key_hits` passes. Its corpus covers sk-, sk-ant-, AIza, ghu\_, Bearer, x-api-key and cookies (CCS-138)._

## Child tickets

p3-endpoint-table, p3-runtime-config-snapshot, p3-router-lock-db-read, p3-route-plan, p3-attempt-recorder, p3-failover-flapping, p3-recovery-strategy, p3-request-pipeline-stages, p3-header-hop-by-hop, p3-header-case-per-request, p3-breaker-sliding-window, p3-usage-ir, p3-stop-reason-table, p3-sse-decoder, p3-stream-traits-taps, p3-bug-report-capture, p3-trace-inspector, p3-usage-parity-badge, p3-breaker-ui-probe, p3-retry-after-cooldown, p3-session-sticky-routing, p3-deps-dedupe-http

## Epic closure audit

Audited 2026-10-30 on `claude/funny-carson-9c7j3e` (c0c770a, after the P4/P5 and lane-2 merges). The audit changed no code.

| Ticket  | Key                        | Status |
| ------- | -------------------------- | ------ |
| CCS-123 | p3-endpoint-table          | Done   |
| CCS-124 | p3-runtime-config-snapshot | Done   |
| CCS-125 | p3-router-lock-db-read     | Done   |
| CCS-126 | p3-route-plan              | Done   |
| CCS-127 | p3-attempt-recorder        | Done   |
| CCS-128 | p3-failover-flapping       | Done   |
| CCS-129 | p3-recovery-strategy       | Done   |
| CCS-130 | p3-request-pipeline-stages | Done   |
| CCS-131 | p3-header-hop-by-hop       | Done   |
| CCS-132 | p3-header-case-per-request | Done   |
| CCS-133 | p3-breaker-sliding-window  | Done   |
| CCS-134 | p3-usage-ir                | Done   |
| CCS-135 | p3-stop-reason-table       | Done   |
| CCS-136 | p3-sse-decoder             | Done   |
| CCS-137 | p3-stream-traits-taps      | Done   |
| CCS-138 | p3-bug-report-capture      | Done   |
| CCS-139 | p3-trace-inspector         | Done   |
| CCS-140 | p3-usage-parity-badge      | Done   |
| CCS-141 | p3-breaker-ui-probe        | Done   |
| CCS-142 | p3-retry-after-cooldown    | Done   |
| CCS-143 | p3-session-sticky-routing  | Done   |
| CCS-144 | p3-deps-dedupe-http        | Done   |

**Done.** All 22 child tickets are Done. The request path now runs from the ClientEndpoint table (CCS-123) to a single RoutePlan per attempt (CCS-126), then through the pipeline/ stages (CCS-130) with a RecoveryStrategy list in place of nested forward() calls (CCS-129). Runtime config comes from an ArcSwap snapshot, so a warm request does 0 DB reads (CCS-124). Cache arithmetic lives in UsageIR (CCS-134) and stop-reason mapping in one table (CCS-135). Stream taps use constant memory (CCS-137). Bug-report capture redacts keys (CCS-138). Measured today: `proxy_e2e` 37/37 and `proxy_golden` 18/18 on Linux, handlers.rs 1050 lines, the longest pipeline/ function 113 lines, one production `forward()` call site, and the redaction and UsageTap tests both pass. Three exit criteria are met.

**Remaining.**

1. **forwarder.rs is 627 lines; the limit is 600.** Feature tickets added after CCS-130 pushed it back over: the trace inspector (CCS-139), Retry-After cooldown (CCS-142), sticky sessions (CCS-143) and the Gemini CLI bridge (CCS-187). To fix it, move about 30 lines out of forwarder.rs into `pipeline/`, for example the Retry-After/cooldown and sticky-session bookkeeping in `forward_with_retry_inner`/`send_with_recovery`, or the per-attempt trace recording. If `forward()` (158 lines) ever moves into `pipeline/`, it must first be split below 150. This needs a small code ticket.
2. **proxy_e2e on 3 OSes.** CI covers this: the `ci.yml` `backend` matrix runs `cargo test` on ubuntu-22.04, windows-latest and macos-latest. No green macOS/Windows run is on record because GitHub has no ci.yml run on the default branch yet. Closing this needs one green CI run on macOS and Windows after merge.
3. **p50 versus the P0 baseline.** This cannot be decided on today's shared, loaded host. Debug p50 measured 3.11–3.50 ms against the P0 baseline of 2.87 ms. Allocations per request rose from 582 to 815 after CCS-124 because later features (capture, traces, sticky routing) added work per request. Re-measure on an idle machine with interleaved A/B runs against fc90858 (the method CCS-124 used): 5+ runs each of `cargo test --test proxy_e2e perf_baseline -- --nocapture`. If p50 has regressed, trim the per-request allocations.

### Remaining after triage

- Blocked: proxy_e2e and the golden suite green on macOS and Windows. Needs a windows-latest/macos-latest run of the ci.yml backend matrix, or real machines (cross-compiling to windows-gnu/apple fails on aws-lc-sys).
- Follow-up [CCS-230](../P3-proxy-request-path/CCS-230-p3-proxy-forwarder-under-600.md): Shrink proxy/forwarder.rs to 600 lines or fewer.
- Follow-up [CCS-231](../P3-proxy-request-path/CCS-231-p3-proxy-p50-remeasure.md): Re-measure p50 proxy overhead against the P0 baseline and trim hot-path allocations if it regressed.
