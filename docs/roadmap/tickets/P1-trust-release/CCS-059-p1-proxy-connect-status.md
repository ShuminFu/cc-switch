# CCS-059: [proxy] Parse the CONNECT status line instead of substring-matching " 200 "

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p3-low`, `area:proxy`

> **Ticket key:** `p1-proxy-connect-status` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding proxy-connect-200-check, partially real. `contains(" 200 ")` rejects `HTTP/1.1 200\r\n` and wrongly accepts `HTTP/1.1 502 Upstream 200 failed`. 407 detection has the same weakness.

## Evidence
- src-tauri/src/proxy/hyper_client.rs:515-534 (:523)

## Proposed change
Add `fn parse_connect_status(line) -> Option<u16>` that checks for an HTTP/ prefix and parses the second token. 2xx means success, 407 maps to AuthError, anything else to ForwardFailed.

## Acceptance criteria / verification
- [x] Unit tests: "HTTP/1.1 200 Connection established", "HTTP/1.0 200 OK", "HTTP/1.1 200", "HTTP/1.1 200 " all give 200; both 407 forms give 407; "502 Upstream 200 failed" gives 502
- [x] `cargo test --manifest-path src-tauri/Cargo.toml proxy::hyper_client`

## Dependencies
None.

## Implementation notes
- Added `parse_connect_status(line) -> Option<u16>` in `src-tauri/src/proxy/hyper_client.rs`: requires an `HTTP/` prefix and a 3-digit second token (reason phrase optional). `connect_via_proxy` now treats any 2xx as success, 407 as `AuthError` and everything else (including malformed lines) as `ForwardFailed`.
- Tests (inline `proxy::hyper_client::tests`): `parse_connect_status_reads_second_token` (all four 200 forms, both 407 forms, `502 Upstream 200 failed` gives 502), `parse_connect_status_rejects_malformed_lines`, plus fake-proxy regression tests `connect_via_proxy_accepts_status_without_reason_phrase`, `connect_via_proxy_rejects_502_mentioning_200`, `connect_via_proxy_maps_bare_407_to_auth_error`, which fail under the old `contains(" 200 ")` check.
- Files: `src-tauri/src/proxy/hyper_client.rs`.
- Commit: 3951f1e.
- Verification: `cargo fmt` clean; `cargo clippy --all-targets -- -D warnings` clean; `cargo test --lib proxy::hyper_client` 8 passed / 0 failed; `cargo test --lib` 2125 passed / 0 failed / 3 ignored.
