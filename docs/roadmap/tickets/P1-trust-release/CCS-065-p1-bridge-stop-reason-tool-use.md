# CCS-065: [bridge] Map finish_reason stop to tool_use when tool calls were emitted

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p2-medium`, `area:proxy-bridge`

> **Ticket key:** `p1-bridge-stop-reason-tool-use` · **Phase:** P1 · **Priority:** p2-medium · **Effort:** S · **Depends on:** `p0-golden-testkit-chat-gemini`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding proxy-stop-with-toolcalls. Gemini-compatible, Ollama and vLLM backends return finish_reason "stop" together with tool_calls. Both the streaming and non-streaming paths map that to end_turn, so Claude Code ends the turn without running the tools.

## Evidence
- src-tauri/src/proxy/providers/streaming.rs:701-715 (map_stop_reason), :518, :544-588, :621
- transform.rs:645-659 (the has_tool_use fallback applies only when finish_reason is absent)

## Proposed change
In streaming, when tool_blocks_by_index is non-empty and the mapped reason is end_turn, use tool_use. Do the same in non-streaming when has_tool_use is set. Leave length mapped to max_tokens.

## Acceptance criteria / verification
- [x] Streaming test: tool_calls then finish stop gives stop_reason tool_use; plain text plus stop still gives end_turn
- [x] Non-streaming test: the same
- [x] `cargo test --manifest-path src-tauri/Cargo.toml proxy::providers`

## Dependencies
p0-golden-testkit-chat-gemini

## Implementation notes

**Summary:** Upstreams that send `finish_reason: "stop"` alongside `tool_calls` now produce `stop_reason: "tool_use"` on both Chat->Anthropic paths. Streaming: after `close_open_blocks` runs on the finish chunk, a mapped `end_turn` becomes `tool_use` if any tool block was started (this includes late starts that `close_open_blocks` adds). The check uses the same `started` predicate as the bare-`[DONE]` fallback. Non-streaming: `openai_to_anthropic` maps `"stop"` to `tool_use` when `has_tool_use` is set. `length` still maps to `max_tokens`, and other values are unchanged.

**Files:**
- `src-tauri/src/proxy/providers/streaming.rs`: fix, plus tests `test_streaming_finish_stop_with_tool_calls_maps_to_tool_use`, `test_streaming_finish_stop_text_only_maps_to_end_turn`, `test_streaming_finish_length_with_tool_calls_keeps_max_tokens`
- `src-tauri/src/proxy/providers/transform.rs`: fix, plus tests `test_openai_to_anthropic_finish_stop_with_tool_calls_maps_tool_use`, `test_openai_to_anthropic_finish_length_with_tool_calls_keeps_max_tokens` (the existing `test_openai_to_anthropic_simple` covers text plus `stop` giving `end_turn`)

**Commits:** `dd5c68e` (code and tests), plus a follow-up docs commit for this ticket file.

**Verification:**
- With both fixes temporarily turned off, the two new `stop`+tool_calls tests failed (streaming and non-streaming). With the fixes in place they pass.
- `cargo fmt --manifest-path src-tauri/Cargo.toml`: clean
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: no warnings
- `cargo test --manifest-path src-tauri/Cargo.toml --lib proxy::providers`: 639 passed, 0 failed
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 2158 passed, 0 failed, 3 ignored
