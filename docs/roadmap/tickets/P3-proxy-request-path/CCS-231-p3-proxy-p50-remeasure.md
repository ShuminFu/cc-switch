# CCS-231: [proxy-perf] Re-measure p50 proxy overhead against the P0 baseline and trim hot-path allocations if it regressed

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P3`, `p3-low`, `area:proxy-perf`

> **Ticket key:** `p3-proxy-p50-remeasure` · **Phase:** P3 · **Priority:** p3-low · **Effort:** M · **Depends on:** `p3-proxy-forwarder-under-600`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-004). Source tickets: [CCS-004](../P3-proxy-request-path/CCS-004-epic-p3.md).

## Problem
The P3 criterion "p50 no worse than the P0 baseline" was never confirmed because the shared host was busy. The host is now idle (load 0.14, 4 CPUs). `allocations_per_request` rose from 582 to about 815, so a regression is plausible.

## Proposed change
- In a scratch worktree of fc90858 (P0 baseline) and on HEAD, interleave at least 5 A/B runs of `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_e2e perf_baseline_non_streaming_request -- --nocapture` (`src-tauri/tests/proxy_e2e.rs:1951`), same profile (debug; optionally release). Do not set `CC_SWITCH_UPDATE_PERF_BASELINE`.
- Compare medians. If HEAD is no worse, record the numbers in CCS-004 and tick the criterion.
- If it regressed, remove per-request work done when features are off (e.g. the eager `Vec<&str>` of applied strategies in `forward_with_retry_inner`, `provider.clone()` calls, capture/trace/sticky bookkeeping) by gating it on trace/capture being enabled; re-run the A/B.
- Optional: `cargo test --no-run` for proxy_e2e/proxy_golden, and try `cargo check --tests --target x86_64-pc-windows-gnu` for cfg-gated compile errors (skip if aws-lc-sys still fails). This does not replace the 3-OS run.

## Acceptance criteria / verification
- [ ] A/B medians recorded in CCS-004; HEAD p50 ≤ fc90858 p50.
- [ ] The `db_reads_per_request == 0` and 3x-allocations gates in proxy_e2e stay green.
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_e2e` and `--test proxy_golden` pass.

## Dependencies
CCS-230 (`p3-proxy-forwarder-under-600`).
