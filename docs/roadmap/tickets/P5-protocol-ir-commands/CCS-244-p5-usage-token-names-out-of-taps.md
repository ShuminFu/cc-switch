# CCS-244: [usage] Move cache-token field parsing out of stream taps, session_usage and the provider testkit

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P5`, `p3-low`, `area:usage`

> **Ticket key:** `p5-usage-token-names-out-of-taps` · **Phase:** P5 · **Priority:** p3-low · **Effort:** M · **Depends on:** `p5-usage-token-name-check`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-006, CCS-178). Source tickets: [CCS-006](../P5-protocol-ir-commands/CCS-006-epic-p5.md), [CCS-178](../P5-protocol-ir-commands/CCS-178-p5-codec-responses.md).

## Problem
Apart from the 5 legacy files, three files outside codec/ and usage/ use the token names: `proxy/stream/taps.rs` (4), `services/session_usage.rs` (9), `proxy/providers/testkit.rs` (5). Moving them is the codable alternative to rewording the criterion.

## Proposed change
- taps.rs: move the usage-field extraction into a helper under the usage module and call it from the tap.
- session_usage.rs: move the field parsing into a usage submodule (e.g. `usage/session_log.rs`), or move the file there.
- testkit.rs: move the fixture JSON into codec test fixtures or a usage test-data file.
- Remove the three entries from the CCS-243 allowlist. Add a fixture change or `no-fixture-needed` label (CCS-034).

## Acceptance criteria / verification
- [ ] `grep -rln 'cached_tokens\|cache_creation_input_tokens' src-tauri/src --include=*.rs | grep -v 'codec/\|usage/'` lists only the 5 legacy files.
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden` stays 18/18 (usage_parity_golden_corpus included); `cargo test --lib session_usage` passes.
- [ ] The CCS-243 check passes with the shortened allowlist.

## Dependencies
CCS-243 (`p5-usage-token-name-check`).
