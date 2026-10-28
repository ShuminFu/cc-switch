# CCS-059: [proxy] Parse the CONNECT status line instead of substring-matching " 200 "

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p3-low`, `area:proxy`

> **Ticket key:** `p1-proxy-connect-status` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding proxy-connect-200-check, partially real. `contains(" 200 ")` rejects `HTTP/1.1 200\r\n` and wrongly accepts `HTTP/1.1 502 Upstream 200 failed`. 407 detection has the same weakness.

## Evidence
- src-tauri/src/proxy/hyper_client.rs:515-534 (:523)

## Proposed change
Add `fn parse_connect_status(line) -> Option<u16>` that checks for an HTTP/ prefix and parses the second token. 2xx means success, 407 maps to AuthError, anything else to ForwardFailed.

## Acceptance criteria / verification
- [ ] Unit tests: "HTTP/1.1 200 Connection established", "HTTP/1.0 200 OK", "HTTP/1.1 200", "HTTP/1.1 200 " all give 200; both 407 forms give 407; "502 Upstream 200 failed" gives 502
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml proxy::hyper_client`

## Dependencies
None.
