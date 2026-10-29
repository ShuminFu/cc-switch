# CCS-062: [bridge] Convert array tool_result content properly in Anthropic->OpenAI Chat (text, images, is_error)

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p2-medium`, `area:proxy-bridge`

> **Ticket key:** `p1-bridge-tool-result` · **Phase:** P1 · **Priority:** p2-medium · **Effort:** S · **Depends on:** `p0-golden-testkit-chat-gemini`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding proxy-tool-result-json. Non-string tool_result content is turned into JSON text by canonical_json_string. Text arrives wrapped in JSON, images and PDFs arrive as raw base64 text (megabytes of tokens), and is_error is dropped.

## Evidence
- src-tauri/src/proxy/providers/transform.rs:411-429 (convert_message_to_openai); image conversion at :386-397; no is_error anywhere in transform.rs

## Proposed change
- Join the text blocks.
- For images, put "[image attached]" in the tool message and add a following user message with image_url parts.
- Replace documents with a placeholder.
- Prefix "[ERROR] " when is_error is set.
- Keep canonical_json_string only for unknown block types.

## Acceptance criteria / verification
- [x] Golden fixtures plus unit tests: [{type:text,text:hello}] gives exactly "hello"; text plus image gives no base64 in the tool message and a following user image_url; is_error gives the prefix
- [x] `cargo test --manifest-path src-tauri/Cargo.toml proxy::providers::transform` and `--test proxy_golden`

## Dependencies
p0-golden-testkit-chat-gemini

## Implementation notes
- **Change:** `convert_message_to_openai` (src-tauri/src/proxy/providers/transform.rs) now routes `tool_result` content through a new `tool_result_content_to_text` helper: string content is kept as-is; array text blocks are joined with `\n`; image blocks become `[image attached]` in the tool message and their `image_url` parts are collected and emitted after all tool messages in the same converted user message (prepended to that user message's own parts, so no consecutive user messages and no user message between tool messages); document blocks become `[document omitted]`; `is_error: true` prefixes `[ERROR] `; only unknown block types (and non-array/non-string content) fall back to `canonical_json_string`. Image conversion was extracted into `image_block_to_openai_part` and is shared with regular image blocks (behaviour unchanged).
- **Files:** src-tauri/src/proxy/providers/transform.rs (code + 4 unit tests), src-tauri/tests/fixtures/proxy/anthropic__openai_chat/request_tool_result_blocks/ (new golden fixture: client_request.json, meta.toml, upstream_request.snap).
- **Commits:** 36b8359 (code, tests, fixture), plus the follow-up commit recording these notes and the fixture's source_commit.
- **Tests:** `test_anthropic_to_openai_tool_result_text_blocks_are_joined` (`[{type:text,text:hello}]` -> exactly `"hello"`; previously JSON text), `..._images_move_to_user_message` (no base64 in tool messages, following user message carries image_url), `..._is_error_prefix`, `..._unknown_block_falls_back_to_json`; golden fixture snapshot pins the same behaviour end to end.
- **Verification:**
  - `cargo fmt` clean; `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` clean.
  - `cargo test --lib proxy::providers::transform` -> 286 passed, 0 failed.
  - `cargo test --test proxy_golden` -> 6 passed, 0 failed (new snapshot reviewed).
  - `cargo test --lib` -> 2136 passed, 0 failed, 3 ignored.
- **Review:** All acceptance criteria verified against the code and tests. One gap fixed: `image_block_to_openai_part` only handled base64 sources, so a `{"type":"url"}` image inside a tool_result (which previously reached the upstream as harmless JSON text) would now have become an empty `data:image/png;base64,` image_url and could be rejected upstream. URL sources now pass through as `image_url.url` (base64 behaviour unchanged); added `test_anthropic_to_openai_tool_result_url_image_keeps_url`. Re-verified: fmt/clippy clean, `proxy::providers::transform` 287 passed, `--test proxy_golden` 6 passed, `--lib` 2137 passed, full `cargo test --no-fail-fast` 2293 passed / 0 failed.
