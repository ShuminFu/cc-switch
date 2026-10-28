# CCS-052: [proxy] Fall back from the raw header-case path only on pre-write errors

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p1-high`, `area:proxy`

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
- [ ] Test: a local TcpListener counts connections, reads the full request and replies with garbage. send_request with OriginalHeaderCases returns Err with exactly 1 connection (currently 2).
- [ ] Test: connect refused still triggers the fallback
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml proxy::hyper_client`

## Dependencies
None.
