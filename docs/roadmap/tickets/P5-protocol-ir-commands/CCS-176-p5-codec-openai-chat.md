# CCS-176: [proxy] Migrate the Anthropic<-OpenAI Chat bridge to codecs

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P5`, `p2-medium`, `area:proxy`

> **Ticket key:** `p5-codec-openai-chat` · **Phase:** P5 · **Priority:** p2-medium · **Effort:** L · **Depends on:** `p5-session-side-store`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
Add codec/openai_chat and codec/anthropic. Delete transform.rs and streaming.rs only after golden parity and one release with no regressions.

## Acceptance criteria / verification
- [ ] Golden fixtures for anthropic__openai_chat pass, including the P1 fixes
- [ ] The parity checker reports 0 mismatches
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden`

## Dependencies
p5-session-side-store
