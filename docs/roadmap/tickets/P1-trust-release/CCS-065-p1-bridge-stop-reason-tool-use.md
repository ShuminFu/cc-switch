# CCS-065: [bridge] Map finish_reason stop to tool_use when tool calls were emitted

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p2-medium`, `area:proxy-bridge`

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
- [ ] Streaming test: tool_calls then finish stop gives stop_reason tool_use; plain text plus stop still gives end_turn
- [ ] Non-streaming test: the same
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml proxy::providers`

## Dependencies
p0-golden-testkit-chat-gemini
