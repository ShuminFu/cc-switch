# CCS-111: [proxy] Delete dead proxy abstractions and cut #[allow(dead_code)] from 69 to 15 or fewer

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P2`, `p3-low`, `area:proxy`

> **Ticket key:** `p2-proxy-dead-code-purge` · **Phase:** P2 · **Priority:** p3-low · **Effort:** S · **Depends on:** `p2-proxy-format-types`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
proxy/ carries 69 #[allow(dead_code)] (the plan said 72; corrected per the critique) and obsolete abstractions.

## Evidence
- types.rs:121 ApiFormat, response_handler.rs, HandlerConfig constants, ProviderAdapter::transform_response, ProviderType::needs_transform, get_adapter_for_provider_type

## Proposed change
Delete them. Keep get_circuit_breaker_stats and get_state, which p3-breaker-ui-probe will use.

## Acceptance criteria / verification
- [ ] `grep -rn 'allow(dead_code)' src-tauri/src/proxy | wc -l` is 15 or fewer
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` and the golden tests pass

## Dependencies
p2-proxy-format-types
