# CCS-015: [tests] Add unit tests for untested proxy helpers, failover switch and balance parsing

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p2-medium`, `area:tests`

> **Ticket key:** `p0-untested-module-tests` · **Phase:** P0 · **Priority:** p2-medium · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-untested-modules. 80 of 213 .rs files have no cfg(test). Most bridges are already covered; the real gaps are listed below.

## Evidence
- proxy/providers/codex_chat_common.rs:8-192 (reasoning/function-call helpers)
- proxy/handler_config.rs, proxy/failover_switch.rs (41-133), proxy/server.rs, services/balance.rs

## Proposed change
- Add #[cfg(test)] modules for:
  - extract_reasoning_field_text, extract_reasoning_summary_text, append_reasoning_content, attach_reasoning_content_field, response_function_call_item(_with_namespace), response_item_call_id
  - try_switch dedup
  - handler config constants
  - balance response parsing

## Acceptance criteria / verification
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib proxy::providers::codex_chat_common`
- [ ] `grep -rL 'cfg(test)' --include=*.rs src-tauri/src | wc -l` drops by at least 4

## Dependencies
None.
