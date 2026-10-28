# CCS-055: [proxy] Apply the streaming first-byte timeout on the native-Claude raw path

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p2-medium`, `area:proxy`

> **Ticket key:** `p1-proxy-first-byte-timeout` · **Phase:** P1 · **Priority:** p2-medium · **Effort:** S · **Depends on:** `p1-proxy-raw-send-no-resend`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding proxy-stream-first-byte-timeout. With failover on, streaming requests on the hyper raw path wait for headers using non_streaming_timeout (600 s) rather than streaming_first_byte_timeout (60 s), so failover is delayed by up to about 10 minutes.

## Evidence
- src-tauri/src/proxy/forwarder.rs:2095-2099 (timeout), 2137-2153 (reqwest uses first-byte), 2156-2171 (hyper passes `timeout`); hyper_client.rs:273-278,304-306; should_preserve_exact_header_case forwarder.rs:3141-3156; handler_context.rs:200-210

## Proposed change
Compute `header_timeout` once before the branch (first-byte when streaming and non-zero) and use it on both paths.

## Acceptance criteria / verification
- [ ] Test: a listener that never sends headers, a Claude provider with api_format anthropic, first_byte 1 s and non_streaming 30 s, and stream:true returns ProxyError::Timeout within about 2 s
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml proxy::forwarder`

## Dependencies
p1-proxy-raw-send-no-resend (shared deadline)
