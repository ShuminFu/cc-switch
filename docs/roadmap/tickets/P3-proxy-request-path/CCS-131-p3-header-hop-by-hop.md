# CCS-131: [proxy] Strip hop-by-hop, cookie and vendor-identity headers from forwarded requests

**Status:** Done · **Kind:** security · **Labels:** `security`, `phase:P3`, `p3-low`, `area:proxy`

> **Ticket key:** `p3-header-hop-by-hop` · **Phase:** P3 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding proxy-header-passthrough. The header rebuild forwards unknown headers by default. connection, keep-alive, te, trailer, upgrade, proxy-connection, proxy-authorization and Connection-listed tokens are never stripped, and cookie passes through. Codex IDs (chatgpt-account-id, openai-organization and others) are removed only on the Codex->Anthropic path.

## Evidence
- src-tauri/src/proxy/forwarder.rs:1819-1983 (:1982 default passthrough), 2806-2822 fingerprint list, 3309-3355; response_processor.rs:37-50

## Proposed change
- Strip request hop-by-hop headers and the tokens listed in Connection; always drop cookie.
- Apply fingerprint stripping whenever the target is not the official OpenAI/ChatGPT upstream.
- Encode these rules in HeaderPolicy.

## Acceptance criteria / verification
- [x] Test: for an OpenAI-compatible provider, none of connection, x-foo (Connection-listed), proxy-authorization, cookie, chatgpt-account-id or openai-organization appear upstream; content-type and the adapter auth are present
- [x] `cargo test --manifest-path src-tauri/Cargo.toml proxy::forwarder`

## Dependencies
None (lands into HeaderPolicy if p3-request-pipeline-stages merges first).

## Implementation notes

**Summary.** The rules live in `HeaderPolicy` (`src-tauri/src/proxy/pipeline/headers.rs`, landed by CCS-130):
- `HOP_BY_HOP_REQUEST_HEADERS` (`connection`, `keep-alive`, `proxy-authenticate`, `proxy-authorization`, `proxy-connection`, `te`, `trailer`, `upgrade`) → `Drop` on every route. `content-length` / `transfer-encoding` were already dropped by `HOP_AND_TRACE_HEADERS`.
- `HeaderPolicy::apply` reads the client's `Connection` tokens per request and drops any header those tokens name that would otherwise pass through by default. Headers the policy writes itself (host, credential slot, accept-encoding, user-agent, ...) are not affected. Example: `Connection: authorization` still gets the adapter's auth header.
- `cookie` / `cookie2` → always `Drop`.
- `chatgpt-account-id`, `openai-organization`, `openai-project` → `Drop` unless the upstream host is `api.openai.com` or `chatgpt.com` (case-insensitive, port ignored). The Codex OAuth `ChatGPT-Account-Id` comes from the adapter's auth headers (the `Auth` slot), so it is unaffected. The broader Codex fingerprint set (`originator`, `session_id`, `x-stainless-*`, ...) is still stripped only on Codex→Anthropic. Third-party Responses/Chat relays may rely on `session_id` for prompt caching, so this change only touches account-identity headers.

**Files.** `src-tauri/src/proxy/pipeline/headers.rs`, `src-tauri/src/proxy/forwarder/wire_golden.rs`.

**Tests.**
- `proxy::forwarder::wire_golden::openai_compatible_upstream_never_sees_hop_by_hop_cookie_or_identity_headers`: end-to-end Codex→OpenAI Chat through `forward_once` against a local capture upstream. It asserts that `connection`, `x-foo` (named in Connection), `keep-alive`, `proxy-authorization`, `cookie`, `chatgpt-account-id` and `openai-organization` are absent, and that `content-type: application/json`, the adapter `authorization: Bearer sk-codex-upstream` and an unrelated `x-custom` are present. It was confirmed to fail on the pre-change `headers.rs` ("connection leaked upstream") and pass after.
- Unit tests in `pipeline::headers::tests`: `hop_by_hop_cookie_and_connection_listed_headers_are_dropped`, `connection_listed_names_do_not_remove_policy_written_headers`, `openai_account_identity_headers_reach_only_official_upstreams`.
- The wire golden fixture `upstream_request.expected` is unchanged.

**Verification.**
- `cargo fmt --check` → clean
- `cargo clippy -- -D warnings` and `cargo clippy --tests --lib -- -D warnings` → clean
- `cargo test proxy::forwarder` → 6 passed, 0 failed
- `cargo test --no-fail-fast` (full suite) → 2438 passed, 0 failed (lib: 2273 passed, 3 ignored pre-existing)

**Commits.** b09f15a (code + tests), plus this ticket-note commit.
