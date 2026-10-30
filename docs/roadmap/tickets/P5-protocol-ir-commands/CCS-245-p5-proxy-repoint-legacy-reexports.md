# CCS-245: [proxy] Repoint non-legacy callers from legacy transform re-exports to codec::*

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P5`, `p3-low`, `area:proxy`

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
- [x] `grep -rn 'providers::transform\|providers::streaming\|transform_[a-z_]*::' src-tauri/src --include=*.rs` returns nothing outside `providers/{transform,streaming}*.rs`, `codec/bridge.rs`, `stream/codec.rs` and comments.
- [x] `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast` pass.

## Dependencies
None.

## Implementation notes

**Summary.** Non-legacy code no longer reaches codec helpers through the legacy
`providers::transform*` / `streaming*` modules:

- `proxy/handlers.rs` imports `build_codex_tool_context_from_request` /
  `CodexToolContext` from `codec::responses::tools`,
  `build_anthropic_usage_from_responses` from `codec::responses::usage` and
  `extract_anthropic_tool_schema_hints` from `codec::gemini::tool_calls`.
- `proxy/errors/render.rs` uses `codec::responses::errors::chat_error_to_response_error`.
- `proxy/pipeline/endpoint.rs` uses `codec::gemini::extract_gemini_model`.
- `proxy/providers/claude.rs` and `proxy/providers/testkit.rs` use
  `codec::openai_chat::support::inject_openai_stream_include_usage` (testkit also
  takes the tool-context / schema-hint helpers from `codec::*`).
- `proxy/stream/codec.rs` takes `CodexToolContext` / `AnthropicToolSchemaHints`
  from `codec::*`; only its test-only legacy stream imports (lines ~208-209) stay,
  together with the legacy arms in `codec/bridge.rs`, for the CCS-178 deletion.
- `proxy/pipeline/request_builder.rs` imports
  `providers::transform_claude_request_for_api_format` (a non-legacy function in
  `providers/claude.rs` whose name matched the ticket grep) by name.
- Tests that called legacy converters directly
  (`codec/side_store/tests.rs`, `stream/aggregate.rs`) now go through
  `codec::bridge` and assert for both `CodecEngine::Legacy` and `CodecEngine::Ir`.
- Re-exports that became unused were removed from `transform_codex_chat.rs`
  (`chat_error_to_response_error`) and `transform_gemini.rs`
  (`extract_gemini_model`, `extract_anthropic_tool_schema_hints`); the legacy
  tests in `transform_gemini.rs` / `streaming_gemini.rs` import the latter from
  `codec::gemini::tool_calls`.
- Regression guard: `proxy::providers::tests::legacy_transform_modules_are_only_referenced_by_allowed_files`
  scans `src-tauri/src` and fails on any non-comment reference to a legacy
  module outside `providers/{transform,streaming}*.rs`, `codec/bridge.rs`,
  `codec/bridge/` and `stream/codec.rs`. Run against the pre-change tree
  (same regex/allowlist via a script over `git show HEAD:`) it reports 16
  offending lines; on the new tree it reports none.

No behaviour change and no wire-format change: `no-fixture-needed` (CCS-034) —
apply that label on the PR.

**Files.** `src-tauri/src/proxy/{handlers.rs, errors/render.rs,
pipeline/endpoint.rs, pipeline/request_builder.rs, stream/codec.rs,
stream/aggregate.rs, codec/side_store/tests.rs}`,
`src-tauri/src/proxy/providers/{mod.rs, claude.rs, testkit.rs,
transform_codex_chat.rs, transform_gemini.rs, streaming_gemini.rs}`.

**Commits.** ca05f8f (code + guard test); this ticket update follows in a
separate docs commit.

**Verification.**
- `grep -rn 'providers::transform\|providers::streaming\|transform_[a-z_]*::' src-tauri/src --include=*.rs`
  → only hits in `providers/{transform,streaming}*.rs`, `codec/bridge.rs`,
  `stream/codec.rs` and `//!` doc comments in `codec/`.
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check` → clean.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` → clean.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast` → all
  binaries ok: 3105 passed, 0 failed, 3 ignored (lib: 2858 passed, 0 failed,
  3 ignored).
