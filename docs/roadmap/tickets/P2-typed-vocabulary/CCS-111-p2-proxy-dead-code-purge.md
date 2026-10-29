# CCS-111: [proxy] Delete dead proxy abstractions and cut #[allow(dead_code)] from 69 to 15 or fewer

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P2`, `p3-low`, `area:proxy`

> **Ticket key:** `p2-proxy-dead-code-purge` · **Phase:** P2 · **Priority:** p3-low · **Effort:** S · **Depends on:** `p2-proxy-format-types`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
proxy/ carries 69 #[allow(dead_code)] (the plan said 72; corrected per the critique) and obsolete abstractions.

## Evidence
- types.rs:121 ApiFormat, response_handler.rs, HandlerConfig constants, ProviderAdapter::transform_response, ProviderType::needs_transform, get_adapter_for_provider_type

## Proposed change
Delete them. Keep get_circuit_breaker_stats and get_state, which p3-breaker-ui-probe will use.

## Acceptance criteria / verification
- [x] `grep -rn 'allow(dead_code)' src-tauri/src/proxy | wc -l` is 15 or fewer
- [x] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` and the golden tests pass

## Dependencies
p2-proxy-format-types

## Implementation notes

**Summary.** `#[allow(dead_code)]` under `src-tauri/src/proxy` went from 73 (the count on this branch before the change) to 6. Dead abstractions were deleted outright. Convenience wrappers that only tests (or the `testkit` golden harness) call are now behind `#[cfg(test)]` / `#[cfg(any(test, feature = "testkit"))]` instead of being allowed as dead.

Deleted:
- `proxy/response_handler.rs` (`ResponseType`, `StreamHandler`, `NonStreamHandler`, `ResponseDispatcher`), `proxy/health.rs` (`HealthChecker`), `proxy/providers/models/` (unused Anthropic/OpenAI serde models), `types::ApiFormat`.
- `handler_config::HandlerConfig` and its four `*_HANDLER_CONFIG` constants. The test that checked them now asserts the Codex chat/responses stream filters directly on the parser configs.
- `session::ClientFormat` / `ProxySession` (replaced by `WireFormat` in CCS-110), along with their re-exports and tests.
- `ProviderAdapter::transform_response` (the trait default and the Claude impl), `ProviderType::needs_transform`, `ProviderType::default_endpoint`, `get_adapter_for_provider_type`.
- `http_client::update_proxy` / `is_proxy_enabled`, `error::categorize_error`, `CodexAdapter::is_official_client` and its regex, `AuthInfo::masked_key` / `masked_access_token`.
- From `OAuthCredentials`: the `refresh_token` / `client_id` / `client_secret` fields and `needs_refresh` / `can_refresh`. Parsing still accepts a JSON blob that holds only a refresh token; a new test, `test_parse_oauth_credentials_json_refresh_token_only`, covers that case.
- From `TokenUsage`: `from_openrouter_response`, `from_codex_response_adjusted` and `from_codex_stream_events`. Also `usage::ApiType`, `CostCalculator::try_calculate`, the unread field `CopilotTokenResponse::refresh_in`, and the field `RequestContext::app_type`.
- `transform_gemini::gemini_to_anthropic_with_shadow`. `gemini_to_anthropic` now calls `gemini_to_anthropic_with_shadow_and_hints` directly.

Gated as test-only: `anthropic_to_openai`, `responses_to_chat_completions`, `chat_completion_to_response`, `anthropic_response_to_responses`, `gemini_to_anthropic`, `create_responses_sse_stream_from_{chat,anthropic}`, `AllowResult::used_half_open_permit`, and the `GeminiShadowStore` helpers `new`, `latest_*`, `clear_*`, `session_count`, `read_inner` and `remove_key_from_order`. `anthropic_to_gemini` is gated on `test` or `feature = "testkit"`.

Remaining allowances (6):
- `ProxyError::ProviderUnhealthy`, `ProxyError::StreamIdleTimeout` and `ErrorCategory::ClientAbort`. Match arms in forwarder, handlers and error_mapper still name these variants, but nothing constructs them. Removing them would mean editing forwarder.rs, which the concurrent lane is changing, so they were left.
- The `log_codes.rs` module-level allow, which covers the log code catalog.
- `CircuitBreaker::get_state` and `ProviderRouter::get_circuit_breaker_stats`. The ticket keeps these for p3-breaker-ui-probe.

**Files:** `src-tauri/src/proxy/{mod,types,error,session,health(deleted),response_handler(deleted),handler_config,handler_context,http_client,circuit_breaker,provider_router}.rs`, `src-tauri/src/proxy/usage/{mod,parser,calculator}.rs`, `src-tauri/src/proxy/providers/{mod,adapter,auth,claude,codex,copilot_auth,gemini,gemini_shadow,transform,transform_gemini,transform_codex_chat,transform_codex_anthropic,streaming_codex_chat,streaming_codex_anthropic}.rs`, `src-tauri/src/proxy/providers/models/*` (deleted).

**Commits:** ceda48c (code), plus this ticket-notes commit.

**Verification:**
- `grep -rn 'allow(dead_code)' src-tauri/src/proxy | wc -l` gives 6, which meets the target of 15 or fewer.
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: clean.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`: clean. Also clean with `--features testkit`, and `cargo check --lib` (non-test) gives 0 warnings.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: all passed, 0 failed. The lib suite is 2197 passed / 3 ignored. 36 tests that only exercised deleted code were removed, 1 test was added, and 1 test was rewritten.
- `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden`: 6 passed.
