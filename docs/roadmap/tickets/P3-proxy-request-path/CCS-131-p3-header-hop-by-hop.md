# CCS-131: [proxy] Strip hop-by-hop, cookie and vendor-identity headers from forwarded requests

**Status:** Open · **Kind:** security · **Labels:** `security`, `phase:P3`, `p3-low`, `area:proxy`

> **Ticket key:** `p3-header-hop-by-hop` · **Phase:** P3 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding proxy-header-passthrough. The header rebuild forwards unknown headers by default. connection, keep-alive, te, trailer, upgrade, proxy-connection, proxy-authorization and Connection-listed tokens are never stripped, and cookie passes through. Codex IDs (chatgpt-account-id, openai-organization and others) are removed only on the Codex->Anthropic path.

## Evidence
- src-tauri/src/proxy/forwarder.rs:1819-1983 (:1982 default passthrough), 2806-2822 fingerprint list, 3309-3355; response_processor.rs:37-50

## Proposed change
- Strip request hop-by-hop headers and the tokens listed in Connection; always drop cookie.
- Apply fingerprint stripping whenever the target is not the official OpenAI/ChatGPT upstream.
- Encode these rules in HeaderPolicy.

## Acceptance criteria / verification
- [ ] Test: for an OpenAI-compatible provider, none of connection, x-foo (Connection-listed), proxy-authorization, cookie, chatgpt-account-id or openai-organization appear upstream; content-type and the adapter auth are present
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml proxy::forwarder`

## Dependencies
None (lands into HeaderPolicy if p3-request-pipeline-stages merges first).
