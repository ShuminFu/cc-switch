# CCS-004: [epic] P3 (v3.21): Proxy request path with endpoint table, RoutePlan, attempt pipeline, canonical usage and stream taps

**Status:** Partial · **Kind:** epic · **Labels:** `epic`, `phase:P3`, `p1-high`, `area:proxy`

> **Ticket key:** `epic-p3` · **Phase:** P3 · **Priority:** p1-high · **Effort:** XL · **Depends on:** `epic-p2`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Goal

Make one decision per attempt that drives both request encoding and response decoding. Break up forward_with_retry_inner and forward(). Write the cache-token arithmetic and the stop-reason mapping once each. Ship bug-report capture (moved here from P2) on the new taps. All golden fixtures stay identical.

## Exit criteria

- [ ] Every golden fixture, including `upstream_request.expected`, is byte-identical after normalization, or the diff is a documented fix; `proxy_e2e` is green on 3 OSes — _Re-audit 2026-10-31 (673be73): fixture half met. Changes since the P3 baseline are all documented: CCS-135 renamed the Gemini drift fixture to a parity fixture with an empty diff; CCS-186/187 only add fixtures; CCS-246 (codecEngine defaults to `ir`) regenerated `forwarder/upstream_request.expected`, whose only diff is that `codex_to_anthropic_impersonating_claude_code` no longer forwards `"stream": false` (content-length 333 to 318), a documented IR-engine difference (CCS-178). `proxy_e2e` 37/37 passes on Linux today; `proxy_golden` 18/18 per CCS-246's full run. Unmet: no recorded green run on macOS/Windows. `ci.yml` `backend` runs `cargo test` on ubuntu-22.04/windows-latest/macos-latest, but `gh run list --workflow ci.yml` still returns 404 (ci.yml is not on the default branch)._
- [x] forwarder.rs is 600 lines or fewer; no function in pipeline/ exceeds 150 lines; handlers.rs is 1200 lines or fewer; forward() has exactly one call site — _Re-audit 2026-10-31: met since CCS-230. forwarder.rs is **596** lines (per-attempt trace recording moved to `pipeline/trace_attempt.rs`, `log_outbound_request` to `pipeline/transport.rs`); the longest function in pipeline/ is still `validate_responses_stream_start` at 113 lines (validate.rs:221-333); handlers.rs is 1055; `.forward(` has one production call site (forwarder.rs:381) plus the test shim in forwarder/tests.rs. `forward()` itself stays in forwarder.rs at 158 lines, which the criterion does not limit._
- [x] Failover from a Copilot (`openai_responses`) provider to an Anthropic provider decodes with the plan's format (E2E) — _`proxy_e2e::failover_from_copilot_responses_to_anthropic_decodes_with_the_anthropic_plan` passes (re-run 2026-10-31), along with the two reverse/Responses failover tests (CCS-126). The harness has no Copilot token source, so the Copilot attempt fails before it sends anything; the decode format still comes from the plan._
- [x] Zero DB reads on the hot path (`proxy_e2e` counter); p50 overhead is no worse than the P0 baseline; UsageTap memory does not grow with the event count — _Zero DB reads: `perf_baseline_non_streaming_request` gives `db_reads_per_request: 0.0` and gates on it (passes in today's `proxy_e2e` run). UsageTap: `stream::taps::tests::usage_tap_memory_is_constant_in_event_count` (100k events) passes. p50: met since CCS-231 (2026-10-31, debug, linux-x86_64, 4 CPUs, interleaved A/B against fc90858, 7 pairs per batch). Before the fix HEAD's median p50 was 3.04 ms against 2.58 ms for fc90858. CCS-231 cached the request-log INSERT and skipped regex redaction of plain header values in the always-on trace; after it, three batches gave HEAD/fc90858 median p50 of 2.48/2.83, 2.51/2.60 and 2.63/2.82 ms. `allocations_per_request` went from 815 to 733 (fc90858: 593; gate 3x the recorded 582)._
- [x] Cache-token arithmetic lives only in `UsageIR::from*`; stream and non-stream paths return the same `stop_reason` for each bridge — _The only cache subtraction outside tests is in `usage/ir.rs` (`from_total_input`, which the `UsageIR` constructors use); CCS-243/244 moved the usage token-name literals out of the taps and added a check for them. Every bridge has a parity fixture (CCS-135), and no fixture pins a `stop_reason` diff. The one non-empty `expected_diff` is a content-block layout diff._
- [x] Exported captures contain no keys (redaction corpus test) — _`proxy::capture::tests::capture_export_has_zero_key_hits` passes. Its corpus covers sk-, sk-ant-, AIza, ghu\_, Bearer, x-api-key and cookies (CCS-138)._

## Child tickets

p3-endpoint-table, p3-runtime-config-snapshot, p3-router-lock-db-read, p3-route-plan, p3-attempt-recorder, p3-failover-flapping, p3-recovery-strategy, p3-request-pipeline-stages, p3-header-hop-by-hop, p3-header-case-per-request, p3-breaker-sliding-window, p3-usage-ir, p3-stop-reason-table, p3-sse-decoder, p3-stream-traits-taps, p3-bug-report-capture, p3-trace-inspector, p3-usage-parity-badge, p3-breaker-ui-probe, p3-retry-after-cooldown, p3-session-sticky-routing, p3-deps-dedupe-http

## Epic closure audit

Re-audited 2026-10-31 on `claude/funny-carson-9c7j3e` (673be73, after the follow-up wave and the lane-2 merge). The audit changed no code. Measured with read-only commands plus one `cargo test --test proxy_e2e` run (37/37).

| Ticket  | Key                                      | Status |
| ------- | ---------------------------------------- | ------ |
| CCS-123 | p3-endpoint-table                        | Done   |
| CCS-124 | p3-runtime-config-snapshot               | Done   |
| CCS-125 | p3-router-lock-db-read                   | Done   |
| CCS-126 | p3-route-plan                            | Done   |
| CCS-127 | p3-attempt-recorder                      | Done   |
| CCS-128 | p3-failover-flapping                     | Done   |
| CCS-129 | p3-recovery-strategy                     | Done   |
| CCS-130 | p3-request-pipeline-stages               | Done   |
| CCS-131 | p3-header-hop-by-hop                     | Done   |
| CCS-132 | p3-header-case-per-request               | Done   |
| CCS-133 | p3-breaker-sliding-window                | Done   |
| CCS-134 | p3-usage-ir                              | Done   |
| CCS-135 | p3-stop-reason-table                     | Done   |
| CCS-136 | p3-sse-decoder                           | Done   |
| CCS-137 | p3-stream-traits-taps                    | Done   |
| CCS-138 | p3-bug-report-capture                    | Done   |
| CCS-139 | p3-trace-inspector                       | Done   |
| CCS-140 | p3-usage-parity-badge                    | Done   |
| CCS-141 | p3-breaker-ui-probe                      | Done   |
| CCS-142 | p3-retry-after-cooldown                  | Done   |
| CCS-143 | p3-session-sticky-routing                | Done   |
| CCS-144 | p3-deps-dedupe-http                      | Done   |
| CCS-230 | p3-proxy-forwarder-under-600 (follow-up) | Done   |
| CCS-231 | p3-proxy-p50-remeasure (follow-up)       | Done   |

Related follow-ups outside this epic that touched the proxy path, all Done: CCS-227 (IPv6 guard for proxy tests), CCS-243/244 (usage token names), CCS-245 (legacy proxy re-exports repointed), CCS-246 (codecEngine defaults to `ir`; regenerated the forwarder wire golden, documented above).

**Done.** All 22 child tickets and both P3 follow-ups are Done. The request path runs from the ClientEndpoint table (CCS-123) to one RoutePlan per attempt (CCS-126), then through the pipeline/ stages (CCS-130) with a RecoveryStrategy list instead of nested forward() calls (CCS-129). Runtime config comes from an ArcSwap snapshot, so a warm request does 0 DB reads (CCS-124). Cache arithmetic lives in UsageIR (CCS-134) and stop-reason mapping in one table (CCS-135). Stream taps use constant memory (CCS-137), and bug-report capture redacts keys (CCS-138). CCS-230 brought forwarder.rs to 596 lines, and CCS-231 brought p50 back to or below the P0 baseline. Five of six exit criteria are met.

**Remaining.**

1. **proxy_e2e and proxy_golden green on macOS and Windows.** This needs CI on GitHub: one green run of the `ci.yml` `backend` matrix (windows-latest, macos-latest) after ci.yml lands on the default branch (today `gh run list --workflow ci.yml` returns 404), or a run on real macOS/Windows machines. Cross-compiling here does not work (aws-lc-sys fails for windows-gnu/apple targets). Once that run is green, tick the first criterion and set Status to Done.
2. **PR-time maintainer steps from the follow-ups (do not block the criteria).** CCS-230 needs the `no-fixture-needed` label (CCS-034). CCS-246 carries a fixture change and is held until a maintainer schedules the release that starts the IR soak.
