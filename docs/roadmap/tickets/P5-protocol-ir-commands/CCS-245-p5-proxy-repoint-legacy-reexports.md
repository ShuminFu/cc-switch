# CCS-245: [proxy] Repoint non-legacy callers from legacy transform re-exports to codec::*

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P5`, `p3-low`, `area:proxy`

> **Ticket key:** `p5-proxy-repoint-legacy-reexports` · **Phase:** P5 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-178). Source tickets: [CCS-178](../P5-protocol-ir-commands/CCS-178-p5-codec-responses.md).

## Problem
Deleting the legacy transform/streaming files (CCS-178, release-gated) should be a pure file removal, but non-legacy code still imports helpers through the legacy modules even though they live in `codec/`:
- `proxy/handlers.rs:26` (`build_codex_tool_context_from_request`, `extract_anthropic_tool_schema_hints`) and :511 (`build_anthropic_usage_from_responses`)
- `proxy/errors/render.rs:13,153,227` (`chat_error_to_response_error`)
- `proxy/pipeline/endpoint.rs:172` (`extract_gemini_model`)
- `proxy/providers/claude.rs:406` (`inject_openai_stream_include_usage`)
- `proxy/providers/testkit.rs:32-33`

## Proposed change
- Import from `codec::responses::{tools,usage,errors}`, `codec::gemini` and `codec::openai_chat::support` instead.
- Leave `proxy/stream/codec.rs:209-210` and `codec/bridge.rs:39-40` (the legacy arms) for the deletion.
- Add a fixture change or `no-fixture-needed` label (CCS-034).

## Acceptance criteria / verification
- [ ] `grep -rn 'providers::transform\|providers::streaming\|transform_[a-z_]*::' src-tauri/src --include=*.rs` returns nothing outside `providers/{transform,streaming}*.rs`, `codec/bridge.rs`, `stream/codec.rs` and comments.
- [ ] `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast` pass.

## Dependencies
None.
