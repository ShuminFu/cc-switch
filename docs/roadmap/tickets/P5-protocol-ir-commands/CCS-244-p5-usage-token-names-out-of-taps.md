# CCS-244: [usage] Move cache-token field parsing out of stream taps, session_usage and the provider testkit

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P5`, `p3-low`, `area:usage`

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
- [x] `grep -rln 'cached_tokens\|cache_creation_input_tokens' src-tauri/src --include=*.rs | grep -v 'codec/\|usage/'` lists only the 5 legacy files.
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden` stays 18/18 (usage_parity_golden_corpus included); `cargo test --lib session_usage` passes.
- [x] The CCS-243 check passes with the shortened allowlist.

## Dependencies
CCS-243 (`p5-usage-token-name-check`).

## Implementation notes

**Summary.** Added `proxy/usage/anthropic_fields.rs` with `AnthropicUsageFields::read(&Value)` (the four Anthropic usage counts as `Option<u32>`) and `to_token_usage()` (missing counts as 0). `proxy/stream/taps.rs` (`AnthropicFold` for `message_start` / `message_delta`) and `services/session_usage.rs` (Claude JSONL `message.usage`) now read the counts through it, with the same precedence rules as before; two tap comments that named the fields were reworded. Added the `#[cfg(test)]` module `proxy/usage/test_data.rs` (`chat_usage`, `responses_usage`, `claude_session_line`); the testkit usage-parity / Responses stream fixtures and the six session-log JSONL fixtures in `session_usage.rs` tests are built with it. The three files are removed from `ALLOWLIST` in `scripts/check-usage-token-names.mjs`, which now lists only the 5 legacy files (the check would report them as stale if left in).

No golden fixture under `tests/fixtures/proxy/` changed and no snapshot moved, so the PR takes the `no-fixture-needed` label (CCS-034); that label has to be set on the PR by whoever opens it.

**Files.** `src-tauri/src/proxy/usage/anthropic_fields.rs` (new), `src-tauri/src/proxy/usage/test_data.rs` (new), `src-tauri/src/proxy/usage/mod.rs`, `src-tauri/src/proxy/stream/taps.rs`, `src-tauri/src/services/session_usage.rs`, `src-tauri/src/proxy/providers/testkit.rs`, `scripts/check-usage-token-names.mjs`.

**Commits.** b7835b1 (code), plus the follow-up commit recording these notes.

**Verification.**
- `grep -rln 'cached_tokens\|cache_creation_input_tokens' src-tauri/src --include=*.rs | grep -v 'codec/\|usage/'` lists only `streaming.rs`, `transform.rs`, `transform_codex_anthropic.rs`, `transform_codex_chat.rs`, `transform_responses.rs`.
- `node scripts/check-usage-token-names.mjs`: passes, "5 allowlisted legacy files"; `npx vitest run tests/scripts/checkUsageTokenNames.test.ts`: 5/5; `prettier --check` on the script: clean.
- `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden`: 18 passed (incl. `usage_parity_golden_corpus`).
- `cargo test --lib session_usage`: 56 passed; `cargo test --lib -- anthropic_fields taps testkit`: 27 passed (incl. 2 new `anthropic_fields` tests).
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 2857 passed, 0 failed, 3 ignored.
- `cargo fmt`, `cargo clippy -- -D warnings`, `cargo clippy --all-targets --features testkit -- -D warnings`: clean.

**Review.** Re-checked b7835b1 against each criterion: the acceptance grep lists only the 5 legacy files, `node scripts/check-usage-token-names.mjs` passes with the shortened allowlist, and the tap (`message_start` / `message_delta`) and session-log reads keep the old `as_u64` / `as u32` / missing-as-0 and delta-precedence rules. The rebuilt test fixtures carry the same counts as the old literals (an absent `stop_reason` now serialises as `null`, which the importer already reads as `None`). Re-ran `cargo fmt --check`, both clippy invocations, `--test proxy_golden` (18/18), `--lib` (2857 passed, 0 failed, 3 ignored) and the vitest check (5/5): all clean. The only change in this review is ticking the acceptance checkboxes.
