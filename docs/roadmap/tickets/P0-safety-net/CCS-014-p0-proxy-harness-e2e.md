# CCS-014: [proxy-tests] Add ProxyHarness with a mock upstream and an end-to-end proxy suite with baselines

**Status:** Partial · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p1-high`, `area:proxy-tests`

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
- [ ] At least 8 scenarios, green on ubuntu, macos and windows in CI (10 tests green on Linux locally; macOS/Windows pending the CI `backend` matrix run)
- [x] Baseline numbers committed to docs/dev/perf-baseline.json
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_e2e`

## Dependencies
p0-testenv-paths

## Implementation notes

**Summary.** A testkit-only `proxy_harness` module drives the real proxy end to end:
- `MockUpstream`: a scripted axum server on `127.0.0.1:0` that records every request (method, path, headers, body). Constructors: `spawn(|index, req| reply)`, `always`, `sequence` (repeats the last reply) and `messages` (a unique message id per request, because usage logs are deduplicated by message id). Replies are JSON (`MockReply::json/error/message`) or chunked SSE (`MockReply::sse`).
- `ProxyHarness::spawn(&MockUpstream)` (single provider, failover off) and `ProxyHarness::builder().provider(..).failover(true).circuit(..).spawn()`. The harness runs the real `ProxyServer` (`build_router` → handlers → `forward_with_retry_inner` → `response_processor` / usage logger) on an ephemeral port over `Database::memory()`, with Claude providers pointing at the mocks and `proxy_config` written for the scenario. Inspection helpers: `request_logs`/`wait_for_request_logs` (`proxy_request_logs` rows), `health`/`wait_for_health` (`provider_health`), `status` (proxy counters).
- DB-query counter: the `testkit` feature enables `rusqlite/trace`; the harness installs a trace hook on its connection that counts executed SQL statements per thread (`db_statement_count()`), excluding the harness's own inspection queries. No production code path changes; nothing under `src-tauri/src/proxy/` was touched.

`tests/proxy_e2e.rs` (10 tests, all on the current-thread `#[tokio::test]` runtime with `TestEnv`):
1. two-provider failover on 503 (credentials per provider, health row, failover counter, log row from the second provider);
2. a 400 does not fail over and does not touch provider health;
3. breaker opens at the failure threshold and the open provider is skipped;
4. half-open: a failed probe re-opens, a successful probe closes and routes back to the primary;
5. thinking-signature rectifier (thinking block stripped, same-provider retry, health neutral);
6. thinking-budget rectifier (budget 32000 / max_tokens 64000 on retry);
7. media rectifier (image replaced by `[Unsupported Image]` on retry);
8. SSE passthrough byte-for-byte plus a streaming usage row (input 21 / output 9);
9. `proxy_request_logs` success row (tokens, model, session, non-zero cost) and failure row (5xx, error message);
10. baseline: 50 non-streaming requests after 5 warm-ups. It writes the measurement to `target/tmp/proxy-e2e-perf.json` (the JSON artifact) and gates on `db_statements_per_request` not growing and `allocations_per_request` staying within 3x of `docs/dev/perf-baseline.json` → `proxy_e2e` (allocations counted by a per-thread counting global allocator in the test binary). Latency is recorded only. To refresh, run `CC_SWITCH_UPDATE_PERF_BASELINE=1 cargo test --manifest-path src-tauri/Cargo.toml --test proxy_e2e perf_baseline`, then `npx prettier --write docs/dev/perf-baseline.json`.

Baseline recorded (linux-x86_64, debug): p50 2.87 ms, p95 3.18 ms, 17 SQL statements/request, 593 allocations/request (≈173 KB). The statement and allocation counts were identical across 5 runs.

**Files.** `src-tauri/Cargo.toml` (`testkit = ["rusqlite/trace"]`), `src-tauri/src/lib.rs` (`pub mod proxy_harness` behind `testkit`), `src-tauri/src/proxy_harness.rs` (new), `src-tauri/tests/proxy_e2e.rs` (new), `docs/dev/perf-baseline.json` (`proxy_e2e` key).

**Commits.** `fc90858` test(proxy): add ProxyHarness with a mock upstream and an end-to-end proxy suite with baselines (CCS-014); this ticket record follows as a separate docs commit.

**Verification.**
- `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_e2e`: 10 passed in about 2.4 s.
- Mutation check: making the media rectifier Codex-only and changing the breaker check `failures >= threshold` to `>` made the media, breaker-open and half-open tests fail (3 failed, 7 passed). Both edits were reverted.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: everything green (lib 2078 passed / 3 ignored; all integration binaries pass, including import_export_sync 26/26 and migrations 5/5). In one earlier full run `migrations::every_released_schema_migrates_to_current` failed once. It then passed in 3 isolated runs and the next full run, so it looks like a load-related flake unrelated to this change.
- `cargo fmt --check`, `cargo clippy -- -D warnings` and `cargo clippy --all-targets -- -D warnings`: clean. `npx prettier --check docs/dev/perf-baseline.json`: clean.

**Remaining.**
- The macOS and Windows results need the CI `backend` matrix. It already runs `cargo test` on ubuntu-22.04, windows-latest and macos-latest, so `proxy_e2e` runs there with no workflow change. Platform risks to watch: the allocation count per request (gated at 3x for this reason) and the 1.1 s half-open wait.
- Uploading `src-tauri/target/tmp/proxy-e2e-perf.json` as a CI artifact (an `actions/upload-artifact` step after "Run tests") was left to the CI lane, because `.github/workflows/ci.yml` belongs to the concurrent CI/docs lane.
- `src-tauri/src/proxy/**` was not changed, so the proxy-fixture rule (CCS-034) does not apply.

### Remaining after triage

- Blocked: proxy_e2e green on macOS and Windows in the backend matrix. Needs a windows-latest/macos-latest run of the ci.yml backend matrix, or real machines (cross-compiling to windows-gnu/apple fails on aws-lc-sys).
- Blocked: Confirming the perf artifact actually uploads. Needs a real GitHub Actions run (CI triggers only on main/PRs; the repo has no Actions history).
- Follow-up [CCS-213](../P0-safety-net/CCS-213-p0-ci-proxy-perf-artifact.md): Upload the proxy_e2e perf JSON as a CI artifact from the backend matrix. Done: ci.yml `backend` now has an `Upload proxy e2e perf artifact` step (`if: always()`, artifact `proxy-e2e-perf-${{ matrix.os }}`) after "Run tests". Only the real-run confirmation above stays blocked on GitHub Actions.

### Wine evidence: full Windows suite (2026-10-31)

- Run: `cargo test --target x86_64-pc-windows-gnu --no-fail-fast` under Wine 9.0 (mingw cross-compile, `LANG=LC_ALL=C.UTF-8`, `WINEPATH` set to the target dir), at `bdd666e`. Under Wine, `proxy_e2e` passes 37 of 37 and `proxy_golden` 18 of 18, and the capture and trace suites pass too. Full suite: 3092 passed, 1 failed (the CCS-083 DACL test, a Wine limitation). Cross-compiling to windows-gnu now works once mingw is installed.
- Still needs real runners: macOS, and a windows-latest run of the backend matrix. Wine does not exercise real WebView2, the real filesystem or real performance (for example the half-open timing and allocation gates).
