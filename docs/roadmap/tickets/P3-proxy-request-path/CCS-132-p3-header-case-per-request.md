# CCS-132: [proxy] Capture original header casing per request instead of one peek per connection

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P3`, `p3-low`, `area:proxy`

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
- [ ] E2E with a raw-recording mock: keep-alive request B keeps `X-Bar-B` casing; a header past 8 KB keeps its casing; a header block split across two writes keeps it
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml proxy`

## Dependencies
p0-proxy-harness-e2e
