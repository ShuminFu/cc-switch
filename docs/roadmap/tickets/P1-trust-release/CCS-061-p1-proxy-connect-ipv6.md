# CCS-061: [proxy] Strip IPv6 brackets before TCP connect and TLS server-name parsing

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p3-low`, `area:proxy`

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
