# CCS-049: [proxy] Require a per-install token and reject foreign Host, Origin and non-JSON requests

**Status:** Open · **Kind:** security · **Labels:** `security`, `phase:P1`, `p0-critical`, `area:proxy`

> **Ticket key:** `p1-proxy-inbound-auth` · **Phase:** P1 · **Priority:** p0-critical · **Effort:** M · **Depends on:** `p0-proxy-harness-e2e`, `p0-config-writer-snapshots`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding proxy-no-auth-host-check. The critique moved this into P1.
- The proxy never checks the PROXY_MANAGED placeholder and attaches the user's real upstream key to any request.
- A web page can send a no-cors text/plain POST to 127.0.0.1:<port>/v1/messages, which costs the user quota and submits a blind prompt.
- DNS rebinding can read responses, because there is no Host check.
- 0.0.0.0 (an opt-in UI setting) exposes the keys to the LAN.

## Evidence
- src-tauri/src/proxy/server.rs:94-114 (bind), 291-369 (build_router has only DefaultBodyLimit)
- services/proxy.rs:22 PROXY_TOKEN_PLACEHOLDER only written to client configs; forwarder.rs:1867-1870 replaces the incoming auth headers with real credentials
- handlers.rs:169-175 parses JSON regardless of Content-Type; ProxyPanel.tsx:149-153 accepts 0.0.0.0

## Proposed change
Add an axum middleware in build_router that:
1. rejects a Host that is not loopback or the configured address:port;
2. rejects requests with an Origin header, or with Sec-Fetch-Site other than none/same-origin;
3. requires application/json on POST;
4. requires a per-install random secret, stored in the DB and written to live configs in place of PROXY_MANAGED, in authorization, x-api-key or x-goog-api-key, compared in constant time.

Accept the legacy PROXY_MANAGED only on a loopback bind during migration. Refuse to start on a non-loopback address unless token auth is enforced, and show a UI warning.

## Acceptance criteria / verification
- [ ] Router tests (tower::ServiceExt::oneshot) return 401/403 without reaching the forwarder for each of: a text/plain POST with Origin evil.example; Host evil.example:PORT; a missing token; a wrong token
- [ ] A loopback Host with a valid token reaches the mock upstream
- [ ] Existing takeover configs are rewritten with the new token (config-writer snapshot updated intentionally)
- [ ] Manual: `curl -H 'Host: attacker.test' -H 'Content-Type: text/plain' -d '{}' http://127.0.0.1:PORT/v1/messages` is rejected
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml proxy::` and clippy

## Dependencies
p0-proxy-harness-e2e, p0-config-writer-snapshots
