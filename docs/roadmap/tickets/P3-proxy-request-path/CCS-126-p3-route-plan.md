# CCS-126: [proxy] Resolve a RoutePlan once per attempt and carry it in ForwardResult

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P3`, `p1-high`, `area:proxy`

> **Ticket key:** `p3-route-plan` · **Phase:** P3 · **Priority:** p1-high · **Effort:** M · **Depends on:** `p3-endpoint-table`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
The request format and the response decoding are derived separately, so they can disagree after failover.

## Evidence
- forwarder.rs:2432 resolve_claude_api_format, 1129-1412 booleans; handlers.rs:229, 837-858, 954-975 re-derive

## Proposed change
- Add proxy/routing/plan.rs: RoutePlan {client, upstream, url, AuthStrategy, VendorQuirks, side stores}.
- ForwardResult carries the plan.
- Log the plan for each attempt.

## Acceptance criteria / verification
- [x] New E2E: failover from a Copilot (openai_responses) provider to an Anthropic provider decodes with the plan's format
- [x] Golden fixtures are identical
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_e2e`

## Dependencies
p3-endpoint-table

## Implementation notes

**Summary:** Added `proxy/routing/plan.rs` with `RoutePlan { provider_id, client, upstream, claude_api_format, url, auth: Option<AuthStrategy>, quirks: VendorQuirks, side_stores: SideStores }` and `RoutePlan::response_decode() -> ResponseDecode` (`Passthrough` / `AnthropicFrom(ClaudeApiFormat)` / `ResponsesFromAnthropic` / `ResponsesFromChat`). `RequestForwarder::forward` builds the plan once per attempt (each provider and each rectifier retry), right after the URL is final and the auth strategy is known, and logs it at info level (`路由计划: provider=… client=… upstream=… api_format=… decode=… url=<no query> auth=… quirks=… side_stores=…`). The code after that point (Anthropic headers, header-case preservation, the 2xx Responses validation) reads `plan.claude_api_format`. `forward` returns the plan, and `ForwardResult.claude_api_format` is replaced by `ForwardResult.plan`. `handle_proxied` now dispatches on `plan.response_decode()`. It no longer calls `get_adapter(..).needs_transform(&ctx.provider)`, `get_claude_api_format`, or `should_convert_codex_responses_to_{anthropic,chat}` again. `handle_claude_transform` reads `plan.quirks.codex_oauth` instead of parsing `provider.meta`. The client wire format reaches the forwarder through the new `RequestScope.client_format`, which `RequestContext::create_forwarder` takes from the endpoint table.

Behaviour is unchanged for every existing route. In the old code the handler's re-derivation happened to agree with the forwarder, so no pre-existing E2E failed. The refactor removes that latent coupling, and the new tests pin the plan-driven decode. One deliberate alignment: if a Codex provider ever matched both the chat and the anthropic conversion, the response decoder now follows the request side (chat first). Before, the handler checked anthropic first.

**Files:**
- `src-tauri/src/proxy/routing/{mod.rs,plan.rs}` (new; unit tests for the decode table and for log redaction)
- `src-tauri/src/proxy/forwarder.rs`, `handlers.rs`, `handler_context.rs`, `mod.rs`
- `src-tauri/src/proxy_harness.rs`: `ProxyHarnessBuilder::provider_with_meta` (testkit only)
- `src-tauri/tests/proxy_e2e.rs`: `failover_from_copilot_responses_to_anthropic_decodes_with_the_anthropic_plan`, `failover_from_responses_provider_to_anthropic_passes_the_message_through`, `failover_from_anthropic_to_responses_provider_converts_the_response`

The harness has no AppHandle, so it has no Copilot token source. In the Copilot scenario, the Copilot attempt (`providerType: github_copilot`, `apiFormat: openai_responses`) therefore fails with a retryable AuthError before sending, and the request fails over to the Anthropic provider. The two Responses scenarios cover a Responses-format attempt that actually reaches the wire, in both failover directions.

**Commits:** `2dff88d` (code + tests), plus the follow-up commit that records these notes.

**Verification:**
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`: clean
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: clean. Also clean with `--tests --features testkit`.
- `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_e2e`: 19 passed, 0 failed (3 new)
- `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden`: 6 passed. No fixture or snapshot files changed.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: lib 2244 passed / 0 failed / 3 ignored. All integration test binaries pass.
