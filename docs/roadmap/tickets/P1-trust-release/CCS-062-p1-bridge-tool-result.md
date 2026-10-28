# CCS-062: [bridge] Convert array tool_result content properly in Anthropic->OpenAI Chat (text, images, is_error)

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p2-medium`, `area:proxy-bridge`

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
- [ ] Golden fixtures plus unit tests: [{type:text,text:hello}] gives exactly "hello"; text plus image gives no base64 in the tool message and a following user image_url; is_error gives the prefix
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml proxy::providers::transform` and `--test proxy_golden`

## Dependencies
p0-golden-testkit-chat-gemini
