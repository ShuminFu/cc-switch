# CCS-060: [proxy] Percent-decode proxy credentials before building Proxy-Authorization

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p3-low`, `area:proxy`

> **Ticket key:** `p1-proxy-connect-userinfo` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding proxy-connect-userinfo-decode. url::Url username() and password() return percent-encoded strings, and GlobalProxySettings always encodes special characters, so proxy auth fails for passwords that contain @ : # % / ? or non-ASCII.

## Evidence
- src-tauri/src/proxy/hyper_client.rs:465-470; src/components/settings/GlobalProxySettings.tsx:50-66

## Proposed change
Extract `fn proxy_auth_header(&Url) -> Option<String>` that runs percent_decode_str on both parts (add percent-encoding as an explicit dependency).

## Acceptance criteria / verification
- [ ] Test: `http://us%40er:p%40ss%3Aw0rd@127.0.0.1:8080` gives `Basic base64(us@er:p@ss:w0rd)`
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml proxy::hyper_client`

## Dependencies
None.
