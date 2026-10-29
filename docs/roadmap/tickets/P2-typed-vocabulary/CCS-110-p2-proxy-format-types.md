# CCS-110: [proxy] Introduce WireFormat, ClaudeApiFormat, AppRef and BreakerKey types

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P2`, `p2-medium`, `area:proxy`

> **Ticket key:** `p2-proxy-format-types` · **Phase:** P2 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p0-golden-responses-fixtures`, `p0-proxy-harness-e2e`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Format dispatch uses &str literals, and breaker keys are split on ':', so the compiler checks nothing.

## Evidence
- proxy/providers/claude.rs:38-96 (api_format strings); provider_router.rs:250 split(':'); circuit_breaker.rs:52

## Proposed change
- Add proxy/protocol/format.rs with WireFormat, ClaudeApiFormat (serde from the existing strings), AppRef and BreakerKey.
- provider_router uses From<&AppProxyConfig>.
- Session-id extraction lives only in session.rs.

## Acceptance criteria / verification
- [ ] "openai_responses", "openai_chat" and "gemini_native" appear only in format.rs (10 or fewer occurrences)
- [ ] Golden fixtures and proxy_e2e are unchanged
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden` and clippy --all-targets

## Dependencies
p0-golden-responses-fixtures, p0-proxy-harness-e2e

## Implementation notes

**Summary**
- New `src-tauri/src/proxy/protocol/format.rs` (re-exported from `proxy::protocol`):
  - `WireFormat` (AnthropicMessages / OpenAiChat / OpenAiResponses / GeminiNative) with `from_codex_wire_api` for the Codex `apiFormat` / TOML `wire_api` aliases.
  - `ClaudeApiFormat` (Anthropic / OpenAiChat / OpenAiResponses / GeminiNative): `as_str`, strict `FromStr`, lenient `from_config` (unknown → Anthropic, same as before), `needs_transform`, `wire_format`, and `Serialize`/`Deserialize` to and from the existing config strings.
  - `AppRef` (app id; `From<&str>`, `From<&AppType>`, `From<&AppProxyConfig>`) and `BreakerKey { app, provider_id }`.
- `get_claude_api_format` returns `ClaudeApiFormat`. `transform_claude_request_for_api_format`, `normalize_anthropic_*`, the forwarder (`ForwardResult.claude_api_format`, `resolve_claude_api_format`, `rewrite_claude_transform_endpoint`, `should_preserve_exact_header_case`, `log_prompt_cache_trace`) and the handlers (`handle_claude_transform`, `should_use_claude_transform_streaming`) all dispatch on the enum. `codex.rs` alias matching, `codex_config::CodexCatalogToolProfile::from_api_format` and the Claude Desktop api-format validation parse through these types.
- `ProviderRouter` keys breakers by `HashMap<BreakerKey, _>`. The `split(':')` is gone, `update_app_configs` compares `AppRef` instead of a string prefix, and breaker configs come from `CircuitBreakerConfig::from(&AppProxyConfig)`.
- Session-id extraction lives only in `session.rs`: the new `metadata_session_key` (Copilot prompt-cache key) and `copilot_session_key` (deterministic request/interaction ids) replace the inline copies in `providers/claude.rs` and `forwarder.rs`. `parse_session_from_user_id` is private again.
- Acceptance count: `"openai_responses"` / `"openai_chat"` / `"gemini_native"` appear 5 times in `format.rs`, which is within the limit of 10, and nowhere else under `src-tauri/src/proxy/`, tests included. Outside `proxy/`, crate production code has no other occurrences. The only remaining literals are config-fixture strings in tests in `services/`, `codex_config.rs` and `claude_desktop_config.rs`, which are persisted-config inputs and were left as they are.
- Out of scope, left for CCS-111: removing `ApiFormat`, `response_handler.rs`, the `HandlerConfig` constants and similar dead code.

**Files**
`src-tauri/src/proxy/protocol/{mod.rs,format.rs}` (new); `src-tauri/src/proxy/{mod.rs,forwarder.rs,handlers.rs,provider_router.rs,session.rs}`; `src-tauri/src/proxy/providers/{claude.rs,codex.rs,mod.rs}`; `src-tauri/src/{codex_config.rs,claude_desktop_config.rs,provider.rs}`.

**Commits**
- `892f118` refactor(proxy): introduce WireFormat, ClaudeApiFormat, AppRef and BreakerKey types (CCS-110)

**Tests added**
- `proxy::protocol::format::tests`: 6 tests covering serde/string round-trip, strict and lenient parsing, `needs_transform`, Codex aliases, and a `BreakerKey` whose provider id contains a colon.
- `proxy::session::tests::test_copilot_session_key_priority`.
- `proxy::provider_router::tests::test_update_app_configs_only_touches_that_app`.

**Verification**
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: clean
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`: clean
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: all green. lib 2232 passed / 0 failed / 3 ignored, `proxy_golden` 6/6, `proxy_e2e` 14/14, every other integration target passed.
- Golden fixtures (`src-tauri/tests/fixtures`) and `tests/proxy_e2e.rs` are unchanged (`git diff --stat` shows no changes under `src-tauri/tests/`).
- `grep -rn '"openai_responses"\|"openai_chat"\|"gemini_native"' src-tauri/src/proxy`: matches only in `protocol/format.rs` (5).
