# CCS-175: [proxy] Move the Gemini shadow, Codex chat history and reasoning bridge behind SessionSideStore

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P5`, `p2-medium`, `area:proxy`

> **Ticket key:** `p5-session-side-store` · **Phase:** P5 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p5-ir-codec-core`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
One SessionSideStore trait passed to the codecs. GeminiShadowStore, CodexChatHistoryStore and reasoning_bridge implement it.

## Acceptance criteria / verification
- [ ] Golden fixtures that depend on the side stores (thought signatures, history restore) are unchanged
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden`

## Dependencies
p5-ir-codec-core
