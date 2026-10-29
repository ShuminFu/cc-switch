# CCS-123: [proxy] Build routes from a ClientEndpoint table with one handle_proxied and per-format error renderers

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P3`, `p2-medium`, `area:proxy`

> **Ticket key:** `p3-endpoint-table` · **Phase:** P3 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p2-proxy-format-types`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Five near-duplicate handlers (about 600 lines) and a copied compact Responses handler.

## Evidence
- proxy/handlers.rs:156, 692, 779, 894, 1723; compact copy at 894-985

## Proposed change
- Add proxy/endpoints.rs with the ClientEndpoint table; server.rs generates its routes from it.
- A single handle_proxied(&ClientEndpoint).
- Add proxy/errors/render.rs with one renderer per client WireFormat.

## Acceptance criteria / verification
- [x] Golden fixtures are byte-identical; proxy_e2e is green
- [x] handlers.rs line count shrinks by at least 500
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_e2e --test proxy_golden`

## Dependencies
p2-proxy-format-types

## Implementation notes

**Summary.** `proxy/endpoints.rs` holds the `CLIENT_ENDPOINTS` table of `ClientEndpoint`s: mount paths, route method (POST, or `any` for Gemini), client `WireFormat`, app type, log tag, how the upstream endpoint is derived (inbound path+query with an optional stripped prefix, or a fixed path plus the inbound query), body decoding (plain, or Codex content-encoded), and the Gemini and Claude Desktop flags (empty body allowed, model from URI, gateway auth). `server.rs` builds every proxied route from the table. Only health/status and the two model-list GETs stay as explicit routes. Every route goes to one `handlers::handle_proxied(state, request, &'static ClientEndpoint)`, which replaces `handle_messages(_for_app)`, `handle_claude_desktop_messages`, `handle_chat_completions`, `handle_responses(_for_app)`, `handle_grokbuild_responses`, the copied `handle_responses_compact(_for_app)` / `handle_grokbuild_responses_compact`, and `handle_gemini`. `proxy/errors/render.rs` has `render_forward_error`, which dispatches on the client `WireFormat`. Anthropic Messages and Gemini Native keep the `ProxyError::into_response` shape. OpenAI Chat and Responses keep the enriched Codex error body (`build_codex_proxy_error_response` / `codex_proxy_error_json`). The upstream Chat to Responses error normaliser (`handle_codex_chat_error_response`) moved there too. Wire behaviour, status codes and error bodies are unchanged.

**Files.**
- `src-tauri/src/proxy/endpoints.rs` (new)
- `src-tauri/src/proxy/errors/mod.rs` and `src-tauri/src/proxy/errors/render.rs` (new; the Codex error-JSON unit tests moved here)
- `src-tauri/src/proxy/handlers.rs`: 3191 to 2563 lines (-628)
- `src-tauri/src/proxy/server.rs`, `src-tauri/src/proxy/mod.rs`
- `src-tauri/src/apps/registry.rs`: the route-mount test now checks `CLIENT_ENDPOINTS` instead of grepping `server.rs` text
- `src-tauri/tests/proxy_e2e.rs`: the malformed-JSON 400 test now covers every POST alias the table generates

**Commits.** `2e7744a` (code), plus the follow-up commit that records these notes.

**Verification.**
- `cargo fmt --manifest-path src-tauri/Cargo.toml`: clean
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`: clean
- `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_e2e --test proxy_golden`: 14 + 6 passed, 0 failed. Golden fixtures under `src-tauri/tests/fixtures/proxy` are untouched (byte-identical).
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 2231 passed, 0 failed, 3 ignored. This includes the new `proxy::endpoints::tests::*` (route coverage, unique paths, compact upstream path, Codex body decoding) and `proxy::errors::render::tests::*`.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast` (all targets): 2389 passed, 0 failed.
