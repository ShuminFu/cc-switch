# CCS-061: [proxy] Strip IPv6 brackets before TCP connect and TLS server-name parsing

**Status:** Partial · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p3-low`, `area:proxy`

> **Ticket key:** `p1-proxy-connect-ipv6` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding proxy-connect-ipv6, partially real. Bracketed hosts ("[::1]") are passed to TcpStream::connect and ServerName::try_from. An IPv6-literal upstream proxy never connects. An IPv6 https upstream fails through a proxy. The direct path silently falls back.

## Evidence
- src-tauri/src/proxy/hyper_client.rs:401, 407, 459-479, 484 (CONNECT line :497 correctly keeps brackets)

## Proposed change
Parse the host as an IpAddr (stripping the brackets) and connect with SocketAddr. Use ServerName::IpAddress for IP literals. Keep the brackets in the CONNECT line and in the Host header.

## Acceptance criteria / verification
- [ ] Test: a CONNECT responder on [::1]:0, `connect_via_proxy("http://[::1]:<port>", ...)` succeeds
- [ ] Test: send_raw_request to http://[::1]:<port>/ returns Ok without fallback
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml proxy::hyper_client`

## Dependencies
None.

## Implementation notes

**Summary.** `src-tauri/src/proxy/hyper_client.rs` gained three helpers:
`unbracket_host` (strips `[...]` from URI hosts), `tcp_connect` (IP literals
connect via `SocketAddr`, other hosts resolve by name) and `tls_server_name`
(`ServerName::IpAddress` for IP literals, `DnsName` otherwise). Both the
direct raw path in `send_raw_request` and the proxy connection plus proxy TLS
in `connect_via_proxy` use them now. The `CONNECT host:port` line and the
`Host` header still carry the bracketed form.

**Tests added** (in `proxy::hyper_client::tests`):
- `unbracket_host_strips_only_ipv6_brackets`
- `tls_server_name_uses_ip_address_for_literals`: the regression test that
  runs everywhere. `ServerName::try_from("[::1]")` was rejected before this
  change.
- `connect_via_proxy_reaches_ipv6_literal_proxy`: a CONNECT responder on
  `[::1]:0`. It checks that the tunnel is established and that the CONNECT
  line and Host header keep the brackets.
- `send_raw_request_reaches_ipv6_literal_upstream`: calls the raw path
  directly against `http://[::1]:<port>/`, so it cannot pass through the
  fallback, and expects `Ok`.

**Files:** `src-tauri/src/proxy/hyper_client.rs`

**Commits:** `2b6910a` (code + tests)

**Verification:**
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`: clean
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: clean
- `cargo test --manifest-path src-tauri/Cargo.toml --lib proxy::hyper_client`:
  14 passed, 0 failed
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 2131 passed,
  0 failed, 3 ignored

**Remaining (why Partial):** this sandbox's kernel has no IPv6 support:
binding `[::1]` fails with `EAFNOSUPPORT`, os error 97. The two
`[::1]`-listener tests detect that, print `skipping: IPv6 loopback
unavailable`, and return early. They compile and run here, but their IPv6
assertions have not run yet. They need one run on a host that has IPv6
loopback (any standard CI runner or dev machine) to tick the first two
acceptance boxes.

**Review.** The fix is correct: `http::Uri::host` and `url::Url::host_str`
both return bracketed IPv6 hosts, and every connect/server-name site in
`hyper_client.rs` (direct TCP, proxy TCP, proxy TLS, target TLS) now goes
through `tcp_connect` / `tls_server_name`, while the CONNECT line and `Host`
header keep the brackets. One gap was in the tests: the "IPv6 https upstream
through a proxy" path had no end-to-end test that runs without IPv6 loopback.
`send_raw_request_through_proxy_builds_tls_name_for_ipv6_target` now sends
`https://[::1]:8443/` through an IPv4 fake CONNECT proxy and expects the
request to reach the TLS handshake. It fails on the old code with "Invalid
server name" and passes here. Verified: fmt and clippy clean,
`proxy::hyper_client` 15 passed, full `cargo test --no-fail-fast` 2288 passed
and 0 failed. Status stays Partial until the two `[::1]`-listener tests run
on a host with IPv6 loopback.

### Remaining after triage

- Blocked: AC1-AC3: the [::1] tests actually running and passing. Need one run on a host with IPv6 loopback (GitHub-hosted runner or dev machine); this kernel has no IPv6.
- Follow-up [CCS-227](../P1-trust-release/CCS-227-p1-proxy-require-ipv6-guard.md): Fail the [::1] proxy tests instead of skipping when CCS_REQUIRE_IPV6 is set.
