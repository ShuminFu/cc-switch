# CCS-015: [tests] Add unit tests for untested proxy helpers, failover switch and balance parsing

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p2-medium`, `area:tests`

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
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib proxy::providers::codex_chat_common`
- [x] `grep -rL 'cfg(test)' --include=*.rs src-tauri/src | wc -l` drops by at least 4

## Dependencies
None.

## Implementation notes

**Summary.** Added `#[cfg(test)]` modules (38 new unit tests) to four previously untested backend files:

- `proxy/providers/codex_chat_common.rs` (16 tests): `extract_reasoning_field_text` (priority order, `reasoning` object keys, `reasoning_details` string/array/object/nested `parts`, empty/absent), `extract_reasoning_summary_text` (direct fields, string and array `summary`), `append_reasoning_content` (trim, insert, `\n\n` append, replace empty/non-string), `attach_reasoning_content_field` / `attach_optional_reasoning_content_field`, `response_function_call_item(_with_namespace)` (shape, reasoning, empty namespace skipped), `response_item_call_id` (prefers `call_id`, trims, no fallback to `id` when `call_id` is blank), plus `is_empty_value` and the `<think>` split helpers.
- `proxy/failover_switch.rs` (4 tests, in-memory `Database`): `try_switch` returns `Ok(false)` and leaves the in-flight marker alone when the same `app:provider` switch is pending; the marker is cleared after a normal run; a non-taken-over app is skipped; the dedup key is scoped to app type + provider id.
- `proxy/handler_config.rs` (7 tests): parser/handler config constants (app type strings, filter wiring, Codex chat vs responses parser), SSE usage pre-filters, and the Claude/OpenAI/Codex/Gemini model extractors' fallback paths.
- `services/balance.rs` (11 tests): `detect_provider`, `parse_f64_field`, `make_auth_error`, the `get_balance` empty-key/unknown-provider short circuits, and response parsing for DeepSeek, StepFun, SiliconFlow (CN/EN), OpenRouter and Novita. To make parsing testable without network access, the body-to-`UsageResult` logic of each `query_*` function moved into a pure `parse_*_balance(&Value)` helper. Behavior is unchanged (transport/read errors still return `Err`, deterministic failures still return `Ok(success:false)`).

`proxy/server.rs` appears in Evidence but not under Proposed change. It is left alone: it needs a bound listener and full app state, which is out of scope for this S-effort ticket.

**Files:** `src-tauri/src/proxy/providers/codex_chat_common.rs`, `src-tauri/src/proxy/failover_switch.rs`, `src-tauri/src/proxy/handler_config.rs`, `src-tauri/src/services/balance.rs`.

**Commits:** `f9d0270` (tests + balance parse extraction); this ticket-file update follows as a docs commit.

**Verification:**
- `cargo test --manifest-path src-tauri/Cargo.toml --lib proxy::providers::codex_chat_common`: 16 passed, 0 failed.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib -- proxy::providers::codex_chat_common proxy::handler_config proxy::failover_switch services::balance`: 38 passed, 0 failed.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 2064 passed, 0 failed, 2 ignored.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`: clean.
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`: clean.
- `grep -rL 'cfg(test)' --include=*.rs src-tauri/src | wc -l`: 80 before, 76 after (drop of 4).
- Dedup regression check: if the dedup guard in `try_switch` were removed, `try_switch_skips_when_same_switch_is_pending` would fail, because the second call would run and clear the in-flight `claude:p1` marker.
