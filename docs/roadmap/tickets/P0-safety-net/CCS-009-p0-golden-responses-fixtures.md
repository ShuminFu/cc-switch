# CCS-009: [proxy-tests] Add golden fixtures for the Responses bridges and recent regression commits

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p1-high`, `area:proxy-tests`

> **Ticket key:** `p0-golden-responses-fixtures` · **Phase:** P0 · **Priority:** p1-high · **Effort:** M · **Depends on:** `p0-golden-testkit-chat-gemini`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
The Responses bridges (anthropic<->responses, responses->chat) have had the most fix commits and are pinned only by inline tests.

## Evidence
- streaming_responses.rs, streaming_codex_chat.rs, streaming_codex_anthropic.rs inline concat!() cases
- Regression commits 6d316c0, 27ce0a5, 650905a, 613fef7, f991726

## Proposed change
- Using the testkit, add fixtures under anthropic__responses, responses__chat and responses__anthropic.
- Give each regression commit a case whose meta.toml cites the commit.

## Acceptance criteria / verification
- [x] At least 30 new fixtures, for a total of 60 or more across all 5 bridges
- [x] Every regression commit listed has a fixture that fails when its fix is reverted (verify by hand for at least 2)
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden`

## Dependencies
p0-golden-testkit-chat-gemini

## Implementation notes

**Summary**
- `proxy::providers::testkit` now drives all 5 bridges. New `Bridge` variants `AnthropicResponses` (`anthropic__responses`), `ResponsesChat` (`responses__chat`) and `ResponsesAnthropic` (`responses__anthropic`), each with a client and an upstream `Protocol`. The drivers mirror the proxy:
  - `anthropic__responses`: `anthropic_to_responses` (new option `codex_oauth`), `create_anthropic_sse_stream_from_responses`, `responses_to_anthropic`.
  - `responses__chat`: `responses_to_chat_completions_with_reasoning(.., None)`, `create_responses_sse_stream_from_chat_with_context`, `chat_completion_to_response_with_context`. The Codex tool context comes from the client request, as in the handler.
  - `responses__anthropic`: `responses_request_to_anthropic(.., 8192)` (the forwarder default), `create_responses_sse_stream_from_anthropic_with_context`, `anthropic_response_to_responses_with_context`.
  - Not covered, because each depends on provider config or session state: `codex_chat_history` enrichment, provider reasoning config, prompt_cache_key / fast mode, and the forwarder's cache-breakpoint injection and Claude Code system prompt.
- Responses-client invariants reuse the existing invariant names:
  - `event_names`; `message_start`: one `response.created`, as the first event.
  - `block_pairing`: each output item is added once and done once; deltas target only open items; every item is done before `response.completed`; the completed `output` equals the done items. A `response.failed` may abandon open items, like an Anthropic `error`.
  - `index_order` for `output_index`.
  - `tool_ids`: `call_id` and name are non-empty and unique.
  - `tool_input_json`: function_call arguments are a JSON object, except on `incomplete` calls.
  - `reasoning_first`.
  - `terminal`: `response.completed` / `response.incomplete` vs `response.failed` / stream error. `terminal = "completed"` is accepted as an alias of `message_stop`.
- Usage is recorded the way each handler does it: `from_codex_*_auto` for Responses clients and `from_claude_*` for Anthropic clients. `usage_conservation` is now protocol-aware. Anthropic counts fresh input without cache tokens; OpenAI, Gemini and Responses count totals that include cache. The prompt total, output, cache read and (new) cache creation must all match the upstream. Upstream usage for Responses comes from the last response object that carries usage, and a stream answered with one whole JSON body is parsed as that body.
- Parity works for Responses clients too: the done output items (ids ignored), `status` and usage. `item_id` and `previous_response_id` are redacted like other generated ids.
- 91 new fixtures, 149 in total:

  | Bridge | Fixtures | Stream | Non-stream | With client request |
  |---|---|---|---|---|
  | `anthropic__responses` | 33 | 21 | 9 | 5 |
  | `responses__chat` | 30 | 23 | 4 | 7 |
  | `responses__anthropic` | 28 | 19 | 5 | 6 |

  Each bridge has one `parity_text_stop` with no expected diff. `ported_from` names the inline tests in `streaming_responses.rs`, `streaming_codex_chat.rs`, `streaming_codex_anthropic.rs`, `transform_responses.rs`, `transform_codex_chat.rs` and `transform_codex_anthropic.rs`.
- Fixtures per regression commit (`source_commit`):
  - `6d316c0`: 4 (tool identity across empty continuation deltas, parallel order with a late name, unnamed call dropped at finalize, sparse index).
  - `27ce0a5`: 10 (done-only arguments, official reasoning signature before stop, keyless legacy reasoning block, interleaved tool deltas, empty/invalid arguments, encrypted reasoning signature and replay, reasoning-only turn dropped).
  - `650905a`: 29 (failed/incomplete/truncated/whole-JSON streams on both Responses bridges, 2xx error envelopes, cancelled status, request hoisting/dropping/structured tool output, URL image/document).
  - `613fef7`: 3 (request reasoning attachment).
  - `f991726`: 6 (cache_write_tokens in each direction, stream and non-stream).
  - `golden_fixture_inventory` now requires at least 60 fixtures and a fixture citing each of the 5 commits.
- Two invariant skips, each with a `skip_reason`:
  - `responses__chat/stream_late_reasoning_on_tool_call` (`reasoning_first`): reasoning that arrives after the tool call is emitted after the function_call item, and its text is also attached to the item as `reasoning_content`.
  - `anthropic__responses/stream_incomplete_without_status` (`usage_conservation`): the upstream usage has no `input_tokens`, so the Codex parser returns nothing, while the client records `output_tokens=3`.
- Observed and recorded, not fixed: `anthropic__responses/stream_incomplete_without_status` emits `message_start` with an empty `id` and `model` when the upstream sends no `response.created`.

**Files**
`src-tauri/src/proxy/providers/testkit.rs`, `src-tauri/tests/proxy_golden.rs`, `src-tauri/tests/fixtures/proxy/{anthropic__responses,responses__chat,responses__anthropic}/**` (new).

**Commits**
- `b1a88dc` test(proxy): add golden fixtures for the Responses bridges and regression commits (CCS-009)

**Verification (Linux)**
- `INSTA_UPDATE=no cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden`: 6 passed (`golden_anthropic_openai_chat`, `golden_anthropic_gemini`, `golden_anthropic_responses`, `golden_responses_chat`, `golden_responses_anthropic`, `golden_fixture_inventory`), in about 22 s.
- `cargo test --lib`: 2074 passed, 0 failed, 2 ignored. This includes 2 new testkit self-tests for the Responses invariants and cross-protocol usage conservation.
- `cargo fmt --check`: clean. `cargo clippy -- -D warnings` and `cargo clippy --all-targets -- -D warnings`: clean.
- Fixes reverted by hand (then restored). Every listed commit fails its fixtures:
  - `6d316c0`: `state.call_id` assigned even from empty ids. Fails `responses__chat/stream_tool_identity_across_empty_continuation` with `[tool_ids] ... has empty call_id` and a snapshot mismatch.
  - `f991726`: `chat_usage_to_responses_usage` reads only the direct `cache_creation_input_tokens`. Fails `responses__chat/stream_cache_write_tokens` and `nonstream_cache_write_tokens` with `[usage_conservation] cache_creation_tokens=0 but upstream cache_creation_tokens=300` (or `=2`) and a snapshot mismatch.
  - `650905a`: the `if terminated { continue; }` guard removed from `streaming_responses.rs`. Fails `anthropic__responses/stream_late_delta_after_failure` with `[message_start] message_start is event #1` and `[terminal] 3 events follow the terminal event #0`.
  - `27ce0a5`: the done-event argument fallback disabled. Fails `anthropic__responses/stream_tool_done_arguments_without_deltas` (snapshot mismatch).
  - `613fef7`: reasoning attached back to the previous assistant when no tool calls are pending. Fails `responses__chat/request_reasoning_attached_forward` and `request_reasoning_on_final_answer_after_tool_call` (snapshot mismatch).

