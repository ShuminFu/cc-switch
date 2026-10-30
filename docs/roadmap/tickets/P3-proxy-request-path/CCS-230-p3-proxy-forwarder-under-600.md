# CCS-230: [proxy] Shrink proxy/forwarder.rs to 600 lines or fewer

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P3`, `p3-low`, `area:proxy`

> **Ticket key:** `p3-proxy-forwarder-under-600` · **Phase:** P3 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-004). Source tickets: [CCS-004](../P3-proxy-request-path/CCS-004-epic-p3.md).

## Problem
The P3 exit criterion caps `src-tauri/src/proxy/forwarder.rs` at 600 lines; it is 627 (`wc -l`, d962272). The rest of the criterion holds: handlers.rs is 1050 lines, `forward()` has one production call site (forwarder.rs:394) and is 158 lines.

## Proposed change
- Move the per-attempt trace recording in `forward_with_retry_inner` (~258-284: outcome → `trace.succeed_attempt`/`fail_attempt`, upstream_status, applied-strategy Vec) into a helper, e.g. `pipeline/trace_attempt.rs::record_attempt_outcome(trace, &outcome, &recovery)` (~25 lines).
- Move `log_outbound_request` (606-621) into `pipeline/transport.rs` or `request_builder.rs` (~16 lines).
- Optionally move the sticky-session bookkeeping (~292-298) into the failover helpers.
- Do not move `forward()` into `pipeline/` unless it is first split under 150 lines (pipeline/ functions stay ≤150; longest today is `validate_responses_stream_start` at 113).
- Add a fixture change or the `no-fixture-needed` label (CCS-034).

## Acceptance criteria / verification
- [ ] `wc -l src-tauri/src/proxy/forwarder.rs` ≤ 600.
- [ ] `grep -n '\.forward(' src-tauri/src/proxy` still shows one production call site.
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden` (18/18), `--test proxy_e2e` and `--lib proxy::forwarder` pass.
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` is clean.

## Dependencies
None.
