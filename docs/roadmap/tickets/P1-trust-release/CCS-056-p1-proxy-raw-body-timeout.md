# CCS-056: [proxy] Bound non-streaming body reads on the raw path when failover is off

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p3-low`, `area:proxy`

> **Ticket key:** `p1-proxy-raw-body-timeout` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding proxy-raw-body-no-timeout, partially real. When failover is off, body_timeout is ZERO. On the hyper raw path, ProxyResponse::Hyper.bytes() then has no deadline and hangs if the upstream stalls after sending headers. The reqwest path is capped by its 600 s client timeout.

## Evidence
- src-tauri/src/proxy/handlers.rs:512-519, 1096, 1250, 1472; response_processor.rs:89-90,217-224; hyper_client.rs:161-168; http_client.rs:218

## Proposed change
Add a helper `ctx.non_streaming_body_timeout()` that returns the configured value when failover is on and greater than 0, and 600 s otherwise. Use it at every call site.

## Acceptance criteria / verification
- [ ] Test: a listener sends headers and one byte, then sleeps. With tokio::time::pause and advance, read_decoded_body returns ProxyError::Timeout with failover disabled.
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml proxy::`

## Dependencies
None.
