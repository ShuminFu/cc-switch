# CCS-135: [proxy] Add a canonical StopReason table and unify the Gemini mapping

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P3`, `p2-medium`, `area:proxy`

> **Ticket key:** `p3-stop-reason-table` · **Phase:** P3 · **Priority:** p2-medium · **Effort:** S · **Depends on:** `p3-usage-ir`, `p1-bridge-stop-reason-tool-use`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
The stream and non-stream paths map stop reasons differently. The Gemini finishReason drift is pinned as an expected diff.

## Evidence
- streaming_gemini.rs:18 vs transform_gemini.rs:1142; streaming.rs:701-715; transform.rs:645-659

## Proposed change
- One StopReason enum with a table per format, including the tool_use override from P1.
- The Gemini unification ships as its own PR and updates the expected-diff fixture.

## Acceptance criteria / verification
- [x] For every bridge, the golden stream and non-stream outputs return the same stop_reason
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden`

## Dependencies
p3-usage-ir, p1-bridge-stop-reason-tool-use

## Implementation notes

**Summary:** A new module, `proxy::protocol::StopReason`, holds the stop-reason mapping. It has one table per upstream format: `from_chat_finish_reason`, `from_gemini_finish_reason` and `from_responses_status`. Each table returns the Anthropic value through `as_anthropic_str`. The CCS-065 tool_use override lives in the tables: when tool calls were emitted, a mapped `end_turn` becomes `tool_use`, while `max_tokens` and `refusal` stay as they are. For Responses the override applies only to `completed`, as before. The stream and non-stream paths of each Anthropic-client bridge now call the same table:
- Chat: `streaming.rs` and `transform.rs`. The old `streaming::map_stop_reason` is gone.
- Gemini: `streaming_gemini.rs` and `transform_gemini.rs`. The duplicate `transform_gemini::map_finish_reason` is gone. The streaming copy is now a thin wrapper that handles the blocked-prompt `refusal`.
- Responses: `transform_responses::map_responses_stop_reason`, which delegates to the table.

The reverse mappings, from stop reason to Responses status (`map_anthropic_stop_reason_to_status` and `response_status_from_finish_reason`), were already single functions shared by both paths. They are unchanged.

Behaviour changes, both of which remove stream vs non-stream drift:
- Gemini: an unknown finishReason (e.g. `MALFORMED_FUNCTION_CALL`) with a tool call now gives `tool_use` on the non-stream path too. It used to give `end_turn` there.
- Chat non-stream: `content_filter` or an unknown finish_reason with tool calls now gives `tool_use`. This matches what streaming already did.

Golden corpus:
- The pinned drift fixture `anthropic__gemini/drift_finish_reason_unknown_with_tool_call` was renamed to `parity_finish_reason_unknown_with_tool_call`, and its `expected_diff` is now empty.
- New parity fixtures with tool calls, one or more for every bridge:
  - `anthropic__openai_chat/{parity_text_stop, parity_stop_with_tool_call, parity_content_filter_with_tool_call}`. This bridge had no parity fixture before.
  - `anthropic__gemini/parity_function_call_stop`
  - `anthropic__responses/parity_function_call`
  - `responses__chat/parity_tool_call`
  - `responses__anthropic/parity_tool_use`
- `golden_fixture_inventory` now requires every bridge to have at least one fixture with both upstream bodies, which is what makes the parity check run.

**Files:**
- `src-tauri/src/proxy/protocol/stop_reason.rs` (new, with table tests) and `src-tauri/src/proxy/protocol/mod.rs`
- `src-tauri/src/proxy/providers/{streaming.rs, streaming_gemini.rs, transform.rs, transform_gemini.rs, transform_responses.rs}`
- `src-tauri/tests/proxy_golden.rs`
- `src-tauri/tests/fixtures/proxy/**/parity_*` (see above)

**Commits:** `6504afe` (code, fixtures, tests), plus a follow-up docs commit for this ticket file.

**Verification:**
- Regression check: I temporarily restored the old `transform.rs` and `transform_gemini.rs`. `cargo test --test proxy_golden` then failed in `golden_anthropic_gemini` (`parity_finish_reason_unknown_with_tool_call`: parity diff changed) and in `golden_anthropic_openai_chat` (`parity_content_filter_with_tool_call`). With the change in place, both pass.
- `cargo fmt --manifest-path src-tauri/Cargo.toml`: clean
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: no warnings
- `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden`: 6 passed, 0 failed
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 2300 passed, 0 failed, 3 ignored
- Note: the CCS-008 ticket notes still name the old `drift_finish_reason_unknown_with_tool_call` fixture. I left them as a historical record.
