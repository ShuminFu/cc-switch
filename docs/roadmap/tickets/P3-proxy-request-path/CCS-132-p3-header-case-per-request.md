# CCS-132: [proxy] Capture original header casing per request instead of one peek per connection

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P3`, `p3-low`, `area:proxy`

> **Ticket key:** `p3-header-case-per-request` · **Phase:** P3 · **Priority:** p3-low · **Effort:** M · **Depends on:** `p0-proxy-harness-e2e`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding proxy-header-case-peek. Casing comes from a single 8 KB peek per TCP connection and is reused for every keep-alive request. Header blocks over 8 KB, partly arrived, or with more than 128 headers get truncated lists. Values are still correct.

## Evidence
- src-tauri/src/proxy/server.rs:156-186; hyper_client.rs:32-41 (from_raw_bytes ignores Partial), 253-270, 586+

## Proposed change
- Wrap the IO in a recording reader that captures each request's header bytes up to CRLFCRLF into a slot that service_fn reads.
- Alternatively, use a growing peek and trust only a Complete parse.
- Size the httparse headers dynamically.

## Acceptance criteria / verification
- [x] E2E with a raw-recording mock: keep-alive request B keeps `X-Bar-B` casing; a header past 8 KB keeps its casing; a header block split across two writes keeps it
- [x] `cargo test --manifest-path src-tauri/Cargo.toml proxy`

## Dependencies
p0-proxy-harness-e2e

## Implementation notes

**Summary.** The one-off 8 KB `TcpStream::peek` per connection is gone. Each accepted stream is now wrapped in `header_case::RecordingIo`, which passes every byte hyper reads to a per-connection `HeaderCaseRecorder`. The recorder frames the HTTP/1.1 stream itself: it reads the head up to the empty line, then skips the `Content-Length` body or the chunked body (chunk sizes, data, trailers). For each head it queues one `OriginalHeaderCases`, built from a `Complete` httparse parse whose header array is sized to the head's line count. `service_fn` takes the front entry for the request it is handling (`header_case::take_cases`) and checks that the method and request target match. On a mismatch, malformed framing, a head or line over 512 KiB, or an `Upgrade`/`CONNECT` request, the recorder stops recording, and the affected requests get an empty case map. That is the existing hyper-util title-case fallback, so the connection itself is never affected. `OriginalHeaderCases::from_raw_bytes` is now a `#[cfg(test)]` helper with the same dynamic header sizing. It has no production caller any more.

**Files.** `src-tauri/src/proxy/header_case.rs` (new), `src-tauri/src/proxy/server.rs`, `src-tauri/src/proxy/hyper_client.rs`, `src-tauri/src/proxy/mod.rs`, `src-tauri/src/proxy_harness.rs` (adds `RawMockUpstream` and `ProxyHarnessBuilder::provider_url`), `src-tauri/tests/proxy_e2e.rs`.

**Tests.**
- E2E `original_header_casing_is_captured_per_request_on_keep_alive_connections` (tests/proxy_e2e.rs). It sends three requests over one raw keep-alive TCP connection to the real proxy, which forwards them to a raw-recording upstream: A (`X-Foo-A`); B (`X-Bar-B`, a 9 KB `X-Padding` header, then `X-After-Eight-K`); C (written in two writes 50 ms apart, split inside the `X-Split-Header` name). It asserts that the upstream sees each name with its wire casing. With the old `server.rs` it fails: B arrived as `x-bar-b`, `x-padding`, `x-after-eight-k`. With the new code it passes.
- Unit tests in `proxy::header_case::tests` (7): keep-alive per-request casing, a head fed byte by byte, a head over 8 KB with more than 200 headers, chunked bodies with extensions and trailers, mismatch disables recording, upgrade stops recording, oversized head disables recording and frees the buffer.

**Verification.**
- `cargo fmt --check` → clean
- `cargo clippy -- -D warnings` and `cargo clippy --tests -- -D warnings` → clean
- `cargo test --test proxy_e2e original_header_casing` → 1 passed (and 1 failed against the pre-change server.rs)
- `cargo test --lib proxy::header_case` → 7 passed
- `cargo test --no-fail-fast` (full suite, which includes every `proxy` test) → 2446 passed, 0 failed (lib: 2280 passed, 3 ignored pre-existing)

**Commits.** ede01ed (code + tests), plus this ticket-note commit.
