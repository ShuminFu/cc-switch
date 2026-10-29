# CCS-052: [proxy] Fall back from the raw header-case path only on pre-write errors

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p1-high`, `area:proxy`

> **Ticket key:** `p1-proxy-raw-send-no-resend` · **Phase:** P1 · **Priority:** p1-high · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding proxy-raw-send-double. With no upstream proxy, any send_raw_request error, including write, flush, handshake or parse errors after the bytes were sent, triggers the hyper-util fallback, which sends the request again. The upstream can then process a paid LLM call twice.

## Evidence
- src-tauri/src/proxy/hyper_client.rs:268-309 (fallback), 373-439 (send_raw_request error sites 402/409/413 pre-write vs 418/422/431/435 and 679/699 post-write); caller forwarder.rs:2162

## Proposed change
- Use an internal `RawSendError { PreWrite, PostWrite }`.
- Fall back only on PreWrite errors (URI/host, TCP connect, server name, TLS handshake).
- Return PostWrite errors as ForwardFailed so the forwarder's own retry policy decides.
- Share one deadline across the raw attempt and the fallback.

## Acceptance criteria / verification
- [x] Test: a local TcpListener counts connections, reads the full request and replies with garbage. send_request with OriginalHeaderCases returns Err with exactly 1 connection (currently 2).
- [x] Test: connect refused still triggers the fallback
- [x] `cargo test --manifest-path src-tauri/Cargo.toml proxy::hyper_client`

## Dependencies
None.

## Implementation notes
- **Change:** `send_raw_request` now returns an internal `RawSendError { PreWrite, PostWrite }`. URI/host, TCP connect (direct or via CONNECT proxy), server-name and TLS-handshake failures are `PreWrite`; write, flush, hyper handshake and response-parse failures are `PostWrite`. `send_request` falls back to the hyper-util client only on `PreWrite` (and still never when an upstream proxy is configured); `PostWrite` errors are returned as `ProxyError::ForwardFailed` so the forwarder's retry policy decides. The raw attempt and the fallback share a single deadline (`tokio::time::timeout_at`).
- **Files:** `src-tauri/src/proxy/hyper_client.rs` (logic + tests `raw_path_does_not_resend_after_request_was_written`, `raw_path_connect_refused_still_falls_back`).
- **Commits:** `21c0f1a` (code + tests); this ticket update in a follow-up docs commit.
- **Verification:**
  - `cargo test --manifest-path src-tauri/Cargo.toml proxy::hyper_client` → 3 passed, 0 failed.
  - Regression check: with the PostWrite arm temporarily made to fall back (old behaviour), the no-resend test failed with `left: 2, right: 1` connections; passes with the fix.
  - `cargo test --manifest-path src-tauri/Cargo.toml --lib` → 2107 passed, 0 failed, 3 ignored.
  - `cargo fmt --check` clean; `cargo clippy -- -D warnings` clean.
