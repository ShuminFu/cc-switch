# CCS-055: [proxy] Apply the streaming first-byte timeout on the native-Claude raw path

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p2-medium`, `area:proxy`

> **Ticket key:** `p1-proxy-first-byte-timeout` · **Phase:** P1 · **Priority:** p2-medium · **Effort:** S · **Depends on:** `p1-proxy-raw-send-no-resend`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding proxy-stream-first-byte-timeout. With failover on, streaming requests on the hyper raw path wait for headers using non_streaming_timeout (600 s) rather than streaming_first_byte_timeout (60 s), so failover is delayed by up to about 10 minutes.

## Evidence
- src-tauri/src/proxy/forwarder.rs:2095-2099 (timeout), 2137-2153 (reqwest uses first-byte), 2156-2171 (hyper passes `timeout`); hyper_client.rs:273-278,304-306; should_preserve_exact_header_case forwarder.rs:3141-3156; handler_context.rs:200-210

## Proposed change
Compute `header_timeout` once before the branch (first-byte when streaming and non-zero) and use it on both paths.

## Acceptance criteria / verification
- [x] Test: a listener that never sends headers, a Claude provider with api_format anthropic, first_byte 1 s and non_streaming 30 s, and stream:true returns ProxyError::Timeout within about 2 s
- [x] `cargo test --manifest-path src-tauri/Cargo.toml proxy::forwarder`

## Dependencies
p1-proxy-raw-send-no-resend (shared deadline)

## Implementation notes

**Summary:** `RequestForwarder::forward` now computes `header_timeout` once, before
the reqwest/hyper branch: `streaming_first_byte_timeout` when the request is
streaming and that timeout is non-zero, otherwise the non-streaming timeout
(default 600 s). The reqwest path wraps `send()` with it (unchanged behaviour)
and the raw hyper path (`hyper_client::send_request`, whose shared deadline
from CCS-052 covers the raw attempt and the fallback up to response headers)
now receives it instead of the non-streaming timeout. Non-streaming requests
are unaffected.

**Files:**
- `src-tauri/src/proxy/forwarder.rs` — timeout selection + regression test
  `proxy::forwarder::tests::streaming_raw_path_uses_first_byte_timeout_for_headers`
  (silent TCP upstream, Claude provider with `api_format: "anthropic"`,
  `OriginalHeaderCases` present so the raw path is taken, first-byte 1 s,
  non-streaming 30 s, `stream: true` → `ProxyError::Timeout` in < 3 s).

**Commits:** `cd61f55` (code + test); ticket notes in the follow-up docs commit.

**Verification:**
- Regression test fails before the fix (forward still pending after the 10 s
  guard) and passes after (~1.0 s).
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` — clean.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` — clean.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib proxy::forwarder` — 68 passed, 0 failed.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib` — 2112 passed, 0 failed, 3 ignored.
