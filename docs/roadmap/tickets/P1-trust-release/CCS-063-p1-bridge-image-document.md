# CCS-063: [bridge] Handle URL-source images and document blocks in Anthropic->OpenAI Chat

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p2-medium`, `area:proxy-bridge`

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
- [ ] Tests: a URL image gives image_url.url == https://example.com/a.png; a base64 image is unchanged; a document gives non-empty content
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml proxy::providers::transform`

## Dependencies
p0-golden-testkit-chat-gemini
