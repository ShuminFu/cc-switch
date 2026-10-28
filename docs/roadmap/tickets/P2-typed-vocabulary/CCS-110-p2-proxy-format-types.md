# CCS-110: [proxy] Introduce WireFormat, ClaudeApiFormat, AppRef and BreakerKey types

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P2`, `p2-medium`, `area:proxy`

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
