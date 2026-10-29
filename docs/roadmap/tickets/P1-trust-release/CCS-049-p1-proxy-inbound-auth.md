# CCS-049: [proxy] Require a per-install token and reject foreign Host, Origin and non-JSON requests

**Status:** Done · **Kind:** security · **Labels:** `security`, `phase:P1`, `p0-critical`, `area:proxy`

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
- [x] Router tests (tower::ServiceExt::oneshot) return 401/403 without reaching the forwarder for each of: a text/plain POST with Origin evil.example; Host evil.example:PORT; a missing token; a wrong token
- [x] A loopback Host with a valid token reaches the mock upstream
- [x] Existing takeover configs are rewritten with the new token (config-writer snapshot updated intentionally)
- [x] Manual: `curl -H 'Host: attacker.test' -H 'Content-Type: text/plain' -d '{}' http://127.0.0.1:PORT/v1/messages` is rejected
- [x] `cargo test --manifest-path src-tauri/Cargo.toml proxy::` and clippy

## Dependencies
p0-proxy-harness-e2e, p0-config-writer-snapshots

## Implementation notes

**Summary.** New module `src-tauri/src/proxy/inbound_auth.rs` adds an `InboundGuard` axum middleware (`enforce_inbound_auth`), layered in `ProxyServer::build_router` over every route, so a rejected request never reaches a handler or the forwarder. Checks, in order:
1. `Host` must be `localhost` or an IP literal that is loopback or the listen address, and the port must be the actual listen port (port 0 resolves to the bound port). Domain names are rejected, which blocks DNS rebinding. On a 0.0.0.0/:: bind, any IP literal is accepted → 403.
2. Any `Origin` header, or `Sec-Fetch-Site` other than `none`/`same-origin` → 403.
3. POST/PUT/PATCH must be `application/json` → 415 (so a no-cors `text/plain` POST is refused).
4. A per-install token in `authorization` (Bearer or raw), `x-api-key` or `x-goog-api-key`, compared in constant time → 401. `/health` and `/claude-desktop/*` (which checks its own gateway token) skip only this step.

The token is `ccs-proxy-<32 hex>`, stored in `settings.proxy_inbound_token` (`get_or_create_inbound_token`). Takeover code still writes the `PROXY_MANAGED` sentinel. `ProxyService`'s live writers (Claude/Codex/Gemini/Grok) bind it to the token right before writing (`bind_inbound_token`). The Codex official (ChatGPT login) route has `Authorization` reserved for the user's login, so it sends the token through `http_headers = { x-api-key = ... }`, and the forwarder drops that header. Takeover detection, cleanup, the live→provider sync filters, Codex placeholder stripping, Grok `has_proxy_token` and the forwarder's stale-placeholder guards now recognise both the new token and the legacy one (`is_proxy_token` / `contains_proxy_token`). Legacy `PROXY_MANAGED` is accepted only when the bind address is loopback, so CLIs launched before the upgrade keep working until they restart. `ProxyService::start` rewrites taken-over live configs that still hold `PROXY_MANAGED` to the token (`migrate_legacy_proxy_token_in_live`). A non-loopback bind gets a startup warning (`SRV-008`) and fails if no token is available. Rejections are logged as `SRV-007`. ProxyPanel shows a LAN-exposure warning (all 4 locales) when the listen address is not loopback.

`tower` was bumped from 0.4 to 0.5 with `util` so tests can use `tower::ServiceExt::oneshot`. 0.5 is the version axum already pulls in, so the lockfile drops the duplicate 0.4 and adds nothing new.

**Files.** `src-tauri/src/proxy/inbound_auth.rs` (new), `proxy/{server,forwarder,log_codes,mod}.rs`, `services/proxy.rs`, `services/provider/mod.rs` (test), `codex_config.rs`, `grok_config.rs`, `proxy_harness.rs` (sends the token, exposes `router()`/`port()`/`token()`, installs the ring TLS provider like app startup), `tests/proxy_e2e.rs`, `tests/provider_service.rs`, `Cargo.toml`/`Cargo.lock`, `src/components/proxy/ProxyPanel.tsx`, `src/i18n/locales/{en,zh,zh-TW,ja}.json`.

**Commits.** 409351a (implementation), plus the follow-up commit recording these notes.

**Verification.**
- `tests/proxy_e2e.rs::router_rejects_untrusted_requests_before_the_forwarder` drives `harness.router()` with `tower::ServiceExt::oneshot`. Results: text/plain POST + `Origin: https://evil.example` → 403; `Host: evil.example:PORT` → 403; missing token → 401; wrong token → 401; real-looking upstream key → 401. Mock upstream hits stay 0 and no request-log rows are written. Then a loopback Host with the valid token → 200: the upstream receives the provider credential and never sees `x-api-key`.
- `foreign_host_text_plain_post_is_rejected_on_the_wire` is the manual curl item done as an automated test. It sends the raw `POST /v1/messages` with `Host: attacker.test` and `Content-Type: text/plain` over TCP and gets `HTTP/1.1 403`, with 0 upstream hits. It was not repeated by hand against a running desktop app.
- Regression check: with the middleware layer temporarily replaced by a pass-through, both tests fail (the attack request returns `200 OK`). They pass with it restored.
- `proxy::inbound_auth::tests` (14 unit tests): token headers, Origin, Sec-Fetch-Site, content type, foreign/loopback/LAN hosts, wildcard bind, missing/wrong token, legacy token only on loopback, `/health` and `/claude-desktop` exemptions, token persistence, binding.
- `services::proxy::tests::start_with_takeover_writes_per_install_token_instead_of_placeholder` and `proxy_start_migrates_legacy_placeholder_in_taken_over_live_configs` cover existing takeover configs being rewritten with the new token. The CCS-012 config-writer snapshots (`tests/config_writers.rs`) cover provider writers, not takeover output, so no `.snap` changed. Existing takeover assertions in `services/proxy.rs`, `services/provider/mod.rs` and `tests/provider_service.rs` were updated on purpose to expect the per-install token.
- `cargo fmt` clean. `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` clean, and also with `--all-targets --features testkit`.
- `cargo test --manifest-path src-tauri/Cargo.toml proxy::` passes. Full `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: 2248 passed, 0 failed (`--lib` 2094 passed; `proxy_e2e` 12; `provider_service` 33). The perf baseline gate (SQL statements per request) passes unchanged, because the token is cached in the guard.
- Web: `pnpm typecheck` OK; `pnpm format:check` OK. `pnpm test:unit`: 1138/1139 in the full run. The one failure was `tests/integration/App.test.tsx > covers basic provider flows`, a timing flake under concurrent load: it does not touch ProxyPanel and passed 4/4 when rerun alone.

**Residual risk / not verified here.** Claude Desktop's 3P gateway and the Gemini/Grok CLIs were not exercised end-to-end against a real install. If one of them sends an `Origin` or `Sec-Fetch-Site: cross-site` header from a renderer process, the middleware will refuse it.


**Review.** An independent review checked each acceptance criterion against 409351a and confirmed it. It looked for missed `PROXY_MANAGED` call sites and found none: every non-test comparison now goes through `is_proxy_token`/`contains_proxy_token`. It confirmed startup re-applies takeover for enabled apps, which also rebuilds the Codex official route with its `x-api-key` header. It also confirmed the Codex official route drops `x-api-key` before sending upstream. One gap was fixed: the token lived in the ordinary `settings` table, so SQL export/backup and WebDAV/S3 sync carried it off the install, and an import or sync from another device replaced it. A running proxy caches the token, so after such an import the next live write used a token the guard did not know, and that client got 401 until the proxy restarted. `database/backup.rs` now treats `proxy_inbound_token` as local-only. It is left out of every SQL export, and import, sync import and `restore_from_backup` keep the local value. If there is no local value, they drop any imported one rather than adopting another install's token. Test: `database::backup::tests::proxy_inbound_token_stays_with_this_install_across_export_and_import`. Re-verified: fmt; clippy `-D warnings`, also with `--all-targets --features testkit`; full `cargo test --no-fail-fast`, all suites green (`--lib` 2095 passed / 3 ignored, `proxy_e2e` 12, `provider_service` 33, `import_export_sync` 26).
