# CCS-231: [proxy-perf] Re-measure p50 proxy overhead against the P0 baseline and trim hot-path allocations if it regressed

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P3`, `p3-low`, `area:proxy-perf`

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
- [x] A/B medians recorded in CCS-004; HEAD p50 ≤ fc90858 p50.
- [x] The `db_reads_per_request == 0` and 3x-allocations gates in proxy_e2e stay green.
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_e2e` and `--test proxy_golden` pass.

## Dependencies
CCS-230 (`p3-proxy-forwarder-under-600`).

## Implementation notes
- **Method:** a scratch worktree of fc90858 built the P0 `proxy_e2e` binary into the shared `src-tauri/target` (to save disk). A script then ran `perf_baseline_non_streaming_request --exact --nocapture` alternately on the P0 binary and the HEAD binary, 7 pairs per batch, debug profile, linux-x86_64, 4 CPUs. `CC_SWITCH_UPDATE_PERF_BASELINE` was not set. The host was shared: lane 2 was compiling and Wine tests were running, so the 1-minute load was 3.4–7.1, not idle. The load is recorded for each batch. The worktree and its P0-only build outputs were removed afterwards.
- **Before (6c4974d):** HEAD's p50 regressed. Median 3.04 ms (2.82–3.22) against 2.58 ms (2.44–2.65) for fc90858, at load 3.4–3.7. Allocations per request were 815 against 593.
- **Profile:** valgrind callgrind and dhat on both binaries. The test runs on a current-thread runtime, so the spawned usage-log task also counts against latency. Two costs explain the gap:
  1. `repo::usage::insert_proxy_row` ran `conn.execute`, which re-parsed the INSERT and recompiled the three `superseded_by` triggers on `proxy_request_logs` (added after P0) on every request. That cost about 2.6M instructions per request; the usage-log task as a whole went from 68M (P0) to 152M instructions over 55 requests.
  2. The always-on request trace (CCS-139) redacts the client and upstream headers. `redact_text` ran all nine regexes over every header value, about 0.5M instructions and about 50 allocations per request.
- **Change:**
  - `insert_proxy_row` uses `prepare_cached`. The usage-log task dropped from 152M to 42M instructions, and `insert_proxy_row` from 143M to 33M.
  - `redact_text` returns early through `may_contain_secret` when the text contains none of the literals that every rule needs: `"`, `:`, `=`, `_`, `sk-`, `AIza`, or `bearer` in any case. Values like `application/json` or `2023-06-01` no longer go through the regex engine; the regex scans fell from 9 to 3 header values per request. Output is unchanged: if no rule matches the original text, no rule can match after an earlier rule's replacement either.
  - `normalize_key` builds its string in one pass, so one allocation instead of two.
  - Trace and capture behaviour, the forwarder and the gates are unchanged.
- **After (497c171), three batches (HEAD / fc90858 median p50, with range):**
  - load 3.4–3.8: 2.48 ms (2.38–2.77) / 2.83 ms (2.65–3.90)
  - load 6.8–6.9: 2.51 ms (2.39–3.69) / 2.60 ms (2.47–4.23)
  - load 6.9–7.1: 2.63 ms (2.43–3.96) / 2.82 ms (2.45–4.56)
  - HEAD is at or below P0 in every batch. `allocations_per_request` is now 733 (gate: 3 × 582), `db_reads_per_request` 0.0 and `db_statements_per_request` 2.0. Program total under callgrind is 675M instructions, against 800M before and 770M for P0. The numbers are recorded in CCS-004 and that exit criterion is ticked. `docs/dev/perf-baseline.json` is unchanged.
- **Tests:** in `proxy::capture::redact::tests`:
  - `may_contain_secret_never_skips_a_rule_match` generates 20k seeded random strings from fragments without the punctuation literals, including Unicode case-fold variants and JSON escapes. For every string the prefilter skips, it asserts that no `TEXT_RULES` regex matches, and it requires more than 1000 strings on that skip path. If a future rule adds a new required literal, this test fails.
  - `may_contain_secret_accepts_every_rule_shape` covers one sample per rule plus plain header values that pass through unchanged.
- **Files:** `src-tauri/src/database/repo/usage.rs`, `src-tauri/src/proxy/capture/redact.rs`, `docs/roadmap/tickets/P3-proxy-request-path/CCS-004-epic-p3.md`, and this ticket.
- **Commits:** 497c171 (code), plus the follow-up commit that records these notes.
- **Verification:**
  - `cargo test --test proxy_e2e` → 37 passed, including the `db_reads == 0`, statements and 3x-allocation gates in `perf_baseline_non_streaming_request`; `--test proxy_golden` → 18 passed.
  - Full `cargo test --lib` → 2869 passed, 0 failed, 3 ignored. The capture/redact/repo filters gave 36 passed, including the 2 new tests.
  - `cargo clippy -- -D warnings` and `cargo clippy --all-targets -- -D warnings` are clean; `cargo fmt --check` is clean.
- **Not done (optional items):** no release-profile A/B, because of disk space. No `cargo check --target x86_64-pc-windows-gnu`; the change has no cfg-gated code. The host was never idle (load ≥ 3.4 on 4 CPUs), but the A/B was interleaved and HEAD was at or below P0 in all three post-fix batches.
