# CCS-177: [proxy] Migrate the Gemini bridge to codecs

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P5`, `p2-medium`, `area:proxy`

> **Ticket key:** `p5-codec-gemini` · **Phase:** P5 · **Priority:** p2-medium · **Effort:** L · **Depends on:** `p5-codec-openai-chat`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
Add codec/gemini, then delete transform_gemini.rs and streaming_gemini.rs after parity and one release.

## Acceptance criteria / verification
- [ ] anthropic__gemini fixtures pass, including the P1 delta-mode fixtures
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden`

## Dependencies
p5-codec-openai-chat
