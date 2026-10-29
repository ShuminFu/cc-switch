# CCS-050: [proxy] Enforce the request body limit and return 400 for malformed JSON

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p3-low`, `area:proxy`

> **Ticket key:** `p1-proxy-body-limit` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding proxy-body-limit-bypass. Handlers take axum::extract::Request and call body.collect(), which skips DefaultBodyLimit, so the body size is unlimited. Parse and read errors map to 500.

## Evidence
- src-tauri/src/proxy/handlers.rs:169-175, 702-708, 792-798, 907-913, 1733-1743; server.rs:368; error.rs:150/156

## Proposed change
Add one helper using axum::body::to_bytes(body, LIMIT): it returns 413 when the limit is exceeded and maps serde errors to ProxyError::InvalidRequest (400). Use it at all 5 sites.

## Acceptance criteria / verification
- [x] Router test: `{not json` returns 400
- [x] A body over a small test limit returns 413
- [x] `cargo test --manifest-path src-tauri/Cargo.toml proxy::`

## Dependencies
None.

## Implementation notes

**Summary.** Added `read_request_body` / `read_request_body_with_limit` in `proxy/handlers.rs`, built on `axum::body::to_bytes(body, limit)`. Exceeding the limit maps to the new `ProxyError::PayloadTooLarge` (HTTP 413, status-mapper 413, Codex error code `cc_switch_payload_too_large`). Other read errors map to `InvalidRequest` (400). `parse_request_json` maps serde errors to `InvalidRequest` (400) instead of `Internal` (500). All 5 body-reading sites now use the helpers: Claude/Claude Desktop/GrokBuild messages, chat completions, responses, responses compact, and Gemini. The Gemini empty-body GET path is unchanged. The limit is `server::MAX_REQUEST_BODY_BYTES` (200MB), and the `DefaultBodyLimit` layer now reads that same constant.

**Files.** `src-tauri/src/proxy/{error.rs,error_mapper.rs,handlers.rs,server.rs}`, `src-tauri/tests/proxy_e2e.rs`.

**Tests.**
- `proxy::handlers::tests::request_body_over_limit_returns_413`, `..._for_chunked_stream` (no Content-Length), `request_body_within_limit_is_parsed`, `malformed_json_request_body_returns_400`. These use a small-limit router that runs the same helpers.
- `proxy_e2e::router_returns_400_for_malformed_json_on_every_body_reading_route`: the full router (inbound auth included) gets `{not json` on all 5 routes and returns 400 every time, with 0 upstream hits. Before this change the same request returned 500.

**Commits.** cbe6d58 (code + tests). The ticket update is a follow-up commit.

**Verification.**
- `cargo fmt` clean. `cargo clippy -- -D warnings` and `cargo clippy --all-targets -- -D warnings` clean.
- `cargo test --lib proxy::`: 1168 passed, 0 failed, 1 ignored.
- `cargo test --test proxy_e2e`: 13 passed.
- Full `cargo test --no-fail-fast`: 2254 passed, 0 failed (baseline 2230 plus the new tests).

**Out of scope / follow-up.** For Codex, a zstd/gzip request body is decompressed after the limit check, so the decompressed size has no cap of its own (decompression bomb). That needs a bounded decoder in `content_encoding::decompress_body`.
