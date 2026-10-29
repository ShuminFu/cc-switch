# CCS-063: [bridge] Handle URL-source images and document blocks in Anthropic->OpenAI Chat

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p2-medium`, `area:proxy-bridge`

> **Ticket key:** `p1-bridge-image-document` · **Phase:** P1 · **Priority:** p2-medium · **Effort:** S · **Depends on:** `p0-golden-testkit-chat-gemini`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding proxy-image-url-source. Image blocks always become `data:{media};base64,{data}`, so a URL source becomes an empty data URI and the URL is lost. Document blocks hit `_ => {}` and are dropped silently.

## Evidence
- src-tauri/src/proxy/providers/transform.rs:386-398, :446; the correct pattern is in transform_responses.rs:23,54,115,615

## Proposed change
- Branch on source.type: url maps to image_url.url = source.url; base64 or a missing type builds the data URI; empty data is skipped and logged.
- Add a document arm: a text source becomes a text part, a base64 PDF becomes a file part where supported, otherwise insert a placeholder.

## Acceptance criteria / verification
- [x] Tests: a URL image gives image_url.url == https://example.com/a.png; a base64 image is unchanged; a document gives non-empty content
- [x] `cargo test --manifest-path src-tauri/Cargo.toml proxy::providers::transform`

## Dependencies
p0-golden-testkit-chat-gemini

## Implementation notes

**Summary.** In the Anthropic -> OpenAI Chat bridge (`src-tauri/src/proxy/providers/transform.rs`):
- `image_block_to_openai_part` now branches on `source.type`: `url` maps to `image_url.url = source.url`; `base64` or a missing type builds `data:{media_type};base64,{data}` (unchanged output); empty data, empty URL, or an unsupported source type (e.g. `file`) is skipped and logged with `log::warn!` instead of producing an empty data URI. The same helper serves `tool_result` images.
- New `document_block_to_openai_part` plus a `"document"` arm in `convert_message_to_openai`, so documents always produce non-empty content: `text` source -> text part (prefixed with `[document: <title>]` when titled); `content` source -> joined text; base64 `application/pdf` -> Chat Completions `file` part (`file.filename`, `file.file_data` data URI); anything else (URL source, non-PDF base64, empty data, missing source) -> a `[document omitted...]` placeholder text part including title/URL when known, with a warning log.
- `tool_result` documents keep their existing `[document omitted]` placeholder (tool messages are text-only).

**Tests added** (inline `#[cfg(test)]` in `transform.rs`): `test_anthropic_to_openai_url_image_keeps_url` (asserts `image_url.url == https://example.com/a.png`), `test_anthropic_to_openai_base64_image_unchanged`, `test_anthropic_to_openai_empty_image_sources_are_skipped`, `test_anthropic_to_openai_text_document_becomes_text_part`, `test_anthropic_to_openai_content_document_joins_text`, `test_anthropic_to_openai_pdf_document_becomes_file_part`, `test_anthropic_to_openai_unsupported_document_inserts_placeholder`. The document and empty-image tests fail on the previous code (documents were dropped; empty base64 produced `data:image/png;base64,`).

**Files.** `src-tauri/src/proxy/providers/transform.rs`; this ticket file.

**Commits.** `a0215f2` (code + tests); ticket-notes commit follows.

**Verification.**
- `cargo fmt --manifest-path src-tauri/Cargo.toml` -> clean
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` -> no warnings
- `cargo test --manifest-path src-tauri/Cargo.toml --lib proxy::providers::transform` -> 294 passed, 0 failed
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast` -> 2300 passed, 0 failed (includes golden proxy fixtures)
