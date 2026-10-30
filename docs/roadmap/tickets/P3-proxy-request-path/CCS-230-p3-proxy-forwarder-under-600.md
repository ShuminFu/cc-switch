# CCS-230: [proxy] Shrink proxy/forwarder.rs to 600 lines or fewer

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P3`, `p3-low`, `area:proxy`

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
- [x] `wc -l src-tauri/src/proxy/forwarder.rs` ≤ 600.
- [x] `grep -n '\.forward(' src-tauri/src/proxy` still shows one production call site.
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden` (18/18), `--test proxy_e2e` and `--lib proxy::forwarder` pass.
- [x] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` is clean.

## Dependencies
None.

## Implementation notes
- **Summary:** Pure refactor, no behavior change. The per-attempt trace recording in `forward_with_retry_inner` (outcome → `succeed_attempt`/`fail_attempt`, upstream status, applied recovery strategies) moved to `pipeline/trace_attempt.rs::record_attempt_outcome(trace, Result<u16, &ProxyError>, &[RecoveryStrategy])`, which has 3 new unit tests (success with recovery, upstream error status/reason, transport error without status). `log_outbound_request` moved to `pipeline/transport.rs`. The sticky-session bookkeeping stayed where it was because the line cap was already met. `forward()` was not moved.
- **Files:** `src-tauri/src/proxy/forwarder.rs`, `src-tauri/src/proxy/pipeline/trace_attempt.rs` (new), `src-tauri/src/proxy/pipeline/transport.rs`, `src-tauri/src/proxy/pipeline/mod.rs`.
- **Commits:** 46e8236 (code), plus the follow-up commit that records these notes.
- **Verification:**
  - `wc -l src-tauri/src/proxy/forwarder.rs` → 596 (was 627).
  - `grep -n '\.forward(' src-tauri/src/proxy` → one production call site (forwarder.rs:381).
  - `cargo test --test proxy_golden` → 18 passed; `--test proxy_e2e` → 37 passed; `--lib proxy::forwarder` → 6 passed; `--lib trace_attempt` → 4 passed (3 new tests plus one existing test whose name matches the filter); full `--lib` → 2855 passed, 0 failed, 3 ignored.
  - `cargo clippy --all-targets -- -D warnings` clean; `cargo fmt --check` clean.
- **Remaining (PR-time, maintainer):** no golden fixture changed because this is a behavior-neutral refactor, so the PR needs the `no-fixture-needed` label (CCS-034) before merge.
