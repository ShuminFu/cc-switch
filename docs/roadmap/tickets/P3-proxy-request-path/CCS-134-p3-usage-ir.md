# CCS-134: [usage] Add a canonical UsageIR with from/to converters for each format

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P3`, `p1-high`, `area:proxy-usage`

> **Ticket key:** `p3-usage-ir` · **Phase:** P3 · **Priority:** p1-high · **Effort:** M · **Depends on:** `p0-golden-responses-fixtures`, `p1-usage-input-semantics`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Cache-token arithmetic is reimplemented in every bridge, and the copies drift.

## Evidence
- transform_responses.rs:431, transform_codex_chat.rs:1672, transform_codex_anthropic.rs:144, transform_gemini.rs:1101; UsageParser::from_*

## Proposed change
Add proxy/usage/ir.rs: UsageIR {fresh_input, cache_read, cache_write, output, reasoning, semantics}, with from_<format>/to_<format>. The bridges and UsageParser delegate to it. Add proptest for billable-token conservation.

## Acceptance criteria / verification
- [x] `grep -rn 'cached_tokens' src-tauri/src/proxy/providers` shows only UsageIR call sites
- [x] Golden usage.expected is identical, and the proptest passes
- [x] `cargo test --manifest-path src-tauri/Cargo.toml usage::ir`

## Dependencies
p0-golden-responses-fixtures, p1-usage-input-semantics

## Implementation notes

**Summary.** Added `src-tauri/src/proxy/usage/ir.rs` with `UsageIR { fresh_input, cache_read, cache_write, output, reasoning, semantics }` (`InputSemantics::{Fresh, Total}`), readers `from_anthropic` / `from_responses` / `from_chat_completions` / `from_gemini`, and writers `to_anthropic` / `to_responses` / `to_token_usage`. Cache fields are read in one order for all OpenAI-family formats: direct `cache_read_input_tokens`/`cache_creation_input_tokens` → `input_tokens_details` → `prompt_tokens_details` (the order the billing parser already used). Fresh input = total − cache_read − cache_write, saturating at 0.

Now delegating to the IR:
- `transform_responses::build_anthropic_usage_from_responses` (Responses → Anthropic, streaming and non-streaming; it still passes the `cache_creation` TTL object through)
- `transform::openai_to_anthropic` usage, plus `streaming.rs` `build_anthropic_usage_json` and the message_start usage (the typed `Usage`/`PromptTokensDetails` structs and the `extract_cache_*` helpers are gone)
- `transform_codex_chat::chat_usage_to_responses_usage` (keeps the upstream `total_tokens`, the `completion_tokens_details` passthrough and its existing key order)
- `transform_codex_anthropic::build_responses_usage_from_anthropic`
- `transform_gemini::build_anthropic_usage`
- `TokenUsage::from_claude_response`, `from_codex_response`, `from_openai_response`, `from_gemini_response`, `from_gemini_stream_chunks`. These keep their existing guards for required fields. The Claude SSE delta-merge logic is unchanged.

Drift fixed (each has a regression test that fails on the old code):
- The non-streaming Chat → Anthropic bridge ignored `input_tokens_details.cached_tokens`: `transform::tests::test_openai_to_anthropic_reads_responses_style_cache_details`.
- Chat → Responses ignored a direct `cache_read_input_tokens` when building `input_tokens_details`: `transform_codex_chat::tests::chat_usage_direct_cache_read_field_reaches_responses_details`.
- Responses → Anthropic no longer emits zero-valued cache keys. Like the other Anthropic writers, it only emits them when they are greater than 0.
- Edge case: when upstream cache counts exceed the total input, a `Total`-semantics `TokenUsage.input_tokens` is now recorded as `cache_read + cache_write` (the conserved total) instead of the raw smaller input. The cost is unchanged, because the calculator already clamped fresh input to 0.

Added dev-dependency `proptest = { version = "1", default-features = false, features = ["std"] }` (this changes Cargo.lock).

**Acceptance.**
- `grep -rn 'cached_tokens' src-tauri/src/proxy/providers`: no non-test code matches. The remaining hits are JSON literals and assertions inside `#[cfg(test)]` modules and testkit fixtures, which are upstream payloads rather than arithmetic.
- The golden fixtures under `src-tauri/tests/fixtures/proxy/**` (`usage.snap` and `client_response.snap`) did not change. `cargo test --test proxy_golden` passes 6/6.
- `cargo test --manifest-path src-tauri/Cargo.toml usage::ir` passes 9/9, including 3 proptests: the Anthropic↔Responses round trip, Chat→Anthropic conservation, and Gemini conservation and clamping.

**Files.** `src-tauri/src/proxy/usage/{ir.rs,mod.rs,parser.rs}`, `src-tauri/src/proxy/providers/{streaming.rs,transform.rs,transform_responses.rs,transform_codex_chat.rs,transform_codex_anthropic.rs,transform_gemini.rs}`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`.

**Commits.** `737ace7` (code); this ticket update is in the follow-up docs commit.

**Verification.**
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`: clean
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` and `--all-targets`: clean
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: 2462 passed, 0 failed (lib: 2294 passed before the 2 regression tests were added, 2296 after)

**Review.** Checked each acceptance criterion against `737ace7`, then re-ran fmt, clippy (`--all-targets`), the full suite, `usage::ir` and `proxy_golden`. The IR reads usage in the same field order as the old billing parser. The bridges that delegate to it keep their output shapes and key order, and the golden snapshots did not change. One gap was fixed. After `streaming.rs` changed the chunk `usage` to `Option<Value>`, a chunk whose `usage` was not an object (for example `"usage":"n/a"`) no longer failed to parse. Instead it was converted to zero usage and overwrote the valid usage already stored in `latest_usage` / the pending `message_delta`, so billing would have recorded zero tokens. A non-object `usage` is now treated as missing, both there and in `message_start`. Regression test: `streaming::tests::test_non_object_usage_chunk_does_not_overwrite_earlier_usage`, which fails without the fix. Also noticed but not part of this ticket: `tests/migrations.rs::every_released_schema_migrates_to_current` is flaky. It compares the fixture's `created_at` (from `Utc::now()`) and fails when the run crosses a second boundary. It came in with CCS-011.
