# CCS-058: [proxy] Make mask_url panic-free and never echo credentials from unparseable proxy URLs

**Status:** Open · **Kind:** security · **Labels:** `security`, `phase:P1`, `p3-low`, `area:proxy`

> **Ticket key:** `p1-proxy-mask-url` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding proxy-mask-url-panic. The fallback `&url[..20]` byte-slices and panics on multibyte input. Malformed URLs such as `http//user:pass@host` leak userinfo into error strings and logs.

## Evidence
- src-tauri/src/proxy/http_client.rs:319-335; build_client :232-233; callers commands/global_proxy.rs:21,65,122,148

## Proposed change
When parsing fails, return a constant `<invalid proxy url>`, or strip everything before the last '@' and truncate with chars().take(20).

## Acceptance criteria / verification
- [ ] `mask_url("代理代理代理代理代理代理")` does not panic
- [ ] `mask_url("http//user:pass@proxy.example.com")` has no user/pass
- [ ] `build_client(Some("http//user:secret@host"))` errors without "secret"
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib proxy::http_client`

## Dependencies
None.
