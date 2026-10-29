# CCS-056: [proxy] Bound non-streaming body reads on the raw path when failover is off

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p3-low`, `area:proxy`

> **Ticket key:** `p1-proxy-raw-body-timeout` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding proxy-raw-body-no-timeout, partially real. When failover is off, body_timeout is ZERO. On the hyper raw path, ProxyResponse::Hyper.bytes() then has no deadline and hangs if the upstream stalls after sending headers. The reqwest path is capped by its 600 s client timeout.

## Evidence
- src-tauri/src/proxy/handlers.rs:512-519, 1096, 1250, 1472; response_processor.rs:89-90,217-224; hyper_client.rs:161-168; http_client.rs:218

## Proposed change
Add a helper `ctx.non_streaming_body_timeout()` that returns the configured value when failover is on and greater than 0, and 600 s otherwise. Use it at every call site.

## Acceptance criteria / verification
- [x] Test: a listener sends headers and one byte, then sleeps. With tokio::time::pause and advance, read_decoded_body returns ProxyError::Timeout with failover disabled.
- [x] `cargo test --manifest-path src-tauri/Cargo.toml proxy::`

## Dependencies
None.

## Implementation notes

**Summary.** Added `non_streaming_body_timeout(&AppProxyConfig)` plus `RequestContext::non_streaming_body_timeout()` in `handler_context.rs`. It returns the configured `non_streaming_timeout` when auto-failover is on and the value is > 0, and `DEFAULT_NON_STREAMING_BODY_TIMEOUT` (600s, the same as the reqwest client timeout) otherwise. It never returns zero. All five `read_decoded_body` call sites now use it: `response_processor::handle_non_streaming` and the four non-streaming transform paths in `handlers.rs`. Before this change, with failover off the body read had no deadline, so `ProxyResponse::Hyper.bytes()` hung when the upstream stalled after sending headers. The forwarder's own body read (`forwarder.rs`) is not changed.

Behaviour change: with failover on and `non_streaming_timeout = 0`, body reads are now capped at 600s instead of having no limit. The ticket asks for this ("600 s otherwise").

**Files.**
- `src-tauri/src/proxy/handler_context.rs`: the helper, the constant and the `RequestContext` method
- `src-tauri/src/proxy/handlers.rs`: 4 call sites
- `src-tauri/src/proxy/response_processor.rs`: 1 call site, doc comment, tests
- `src-tauri/Cargo.toml`: dev-dependency `tokio` with the `test-util` feature, needed for `tokio::time::pause`. `Cargo.lock` is unchanged.

**Commits.** `155ea77` (code + tests), plus this ticket-notes commit.

**Tests.**
- `proxy::response_processor::tests::test_read_decoded_body_times_out_on_stalled_hyper_body_with_failover_off`: a TCP listener sends response headers (`Content-Length: 16`) and one body byte, then stalls. After the headers arrive the test calls `tokio::time::pause()`. It then asserts that `read_decoded_body` with the failover-off timeout returns `ProxyError::Timeout` after at least 600s of virtual time, inside an outer 3600s guard. Swapping in the old value (`Duration::ZERO`) makes the test fail with "read_decoded_body must not hang when failover is off: Elapsed(())". With the fix it passes.
- `proxy::response_processor::tests::test_non_streaming_body_timeout_is_never_zero`: checks the helper for (on, 30) → 30s, (on, 0) → 600s and (off, 30) → 600s.

**Verification.**
- `cargo fmt --check`: clean
- `cargo clippy -- -D warnings` and `cargo clippy --tests -- -D warnings`: clean
- `cargo test --lib proxy::`: 1183 passed, 0 failed, 1 ignored
- `cargo test --lib`: 2114 passed, 0 failed, 3 ignored
- `cargo test --no-fail-fast` (full suite including integration tests): 2270 passed, 0 failed

**Review.** The five `read_decoded_body` call sites and the regression test check out. The test fails with the old `Duration::ZERO`. One gap was left: the forwarder's own whole-body reads inside the retry loop still had no deadline when failover is off. These are the non-2xx error body and the 2xx semantic checks `validate_responses_success_response` and `validate_codex_anthropic_success_response`. With failover off, `non_streaming_timeout` is 0, so `prepare_success_response_for_failover` skips buffering and those reads go straight to the raw hyper body. The error-body read was unbounded even with failover on. The fix adds `RequestForwarder::read_body_bounded`, which uses the configured timeout or `DEFAULT_NON_STREAMING_BODY_TIMEOUT` when it is 0, and routes all three reads through it. Test: `proxy::forwarder::tests::bounded_body_reads_time_out_on_stalled_body_with_failover_off` (paused clock, stalled body). It fails without the bound ("Elapsed") and passes with it. Results: fmt/clippy (`--tests`) clean, and the full suite (`cargo test --no-fail-fast`) gives 2271 passed, 0 failed.
