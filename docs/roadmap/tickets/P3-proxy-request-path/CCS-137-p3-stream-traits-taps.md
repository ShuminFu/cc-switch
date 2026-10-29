# CCS-137: [proxy] Wrap converters behind StreamDecoder/Encoder, and add UsageTap, TimingTap and aggregate

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P3`, `p2-medium`, `area:proxy`

> **Ticket key:** `p3-stream-traits-taps` · **Phase:** P3 · **Priority:** p2-medium · **Effort:** L · **Depends on:** `p3-sse-decoder`, `p3-usage-ir`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Usage is re-parsed from a growing Vec of events, and non-stream aggregation is duplicated.

## Evidence
- response_processor.rs:677-790; handlers.rs:1814, 2022

## Proposed change
- Put the existing converters behind StreamDecoder/StreamEncoder traits without rewriting them.
- UsageTap keeps running totals; TimingTap records TTFB and first token.
- Add stream/aggregate.rs.

## Acceptance criteria / verification
- [x] UsageTap memory is constant in the event count (test with 100k events)
- [x] Golden fixtures are identical; handlers.rs is 1200 lines or fewer
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden --test proxy_e2e`

## Dependencies
p3-sse-decoder, p3-usage-ir

## Implementation notes

**Summary.**
- `proxy::stream::codec` adds two traits. `StreamDecoder` turns an upstream byte stream into client SSE and reports `upstream_format`/`client_format`. `StreamEncoder` turns a complete upstream message into client SSE events.
  - The existing converters sit behind these traits unchanged: `ChatToAnthropic`, `ResponsesToAnthropic`, `GeminiToAnthropic`, `ChatToResponses` (which includes Chat-history recording) and `AnthropicToResponses` (both decoder and encoder).
  - `anthropic_decoder(api_format, ..)` replaces the if/else that handlers used to pick a converter.
  - `decode_upstream` returns the client wire format, and that format now selects the usage parser config through `handler_config::parser_config_for`, the same table the passthrough path uses.
- `proxy::stream::taps` adds `UsageTap`, which folds events one at a time into running totals for each `StreamUsageFormat` (Anthropic, OpenAiChat, CodexAuto, Gemini).
  - The value priorities match the old `TokenUsage::from_*_stream_*` functions and the old model extractors exactly. The batch functions are now thin wrappers kept for tests and the testkit.
  - `UsageParserConfig` has a `stream_usage` format field in place of the `stream_parser`/`model_extractor` function pointers.
- `TimingTap` records TTFB (first upstream chunk) and the first token. The first token keeps its existing meaning: the first event the usage collector keeps.
- `SseUsageCollector` holds a `(UsageTap, TimingTap)` instead of a growing `Vec<Value>`.
  - The passthrough tap in `create_logged_passthrough_stream` now frames events with `SseDecoder` instead of `take_sse_block`, and it now also counts a trailing event that has no blank line after it.
- `proxy::stream::aggregate` takes the non-stream SSE aggregators from handlers (`chat_sse_to_response_value`, `responses_sse_to_response_value`), the sniffing and diagnostic helpers, and a new `SseAggregator` + `parse_upstream_json`. That helper replaces three copies of the "JSON, then sniff SSE, then aggregate, then report diagnostics" branch.
- `response_processor` gains `create_usage_collector(.., MissingStreamUsage)`, `transformed_sse_response`, `rebuilt_json_response` and `response_usage_log_task`/`spawn_response_usage_log`. They replace the duplicate `log_usage`, the three stream-collector closures and the two non-stream usage spawns in handlers. `handlers.rs` went from 2543 to 865 lines.
- Intentional minor behaviour changes:
  1. An aggregation failure on the Codex↔Anthropic non-stream path now carries the same content-type/body diagnostics as the other paths.
  2. On the transform streaming paths the attributed model uses the passthrough extractor fallback: the model from usage, then the event model fields, then the request model. Before, those paths went straight from the usage model to the request model.
  3. Transform stream logs use `ctx.tag` (the old Claude path used "Claude/OpenRouter").

**Files.** `src-tauri/src/proxy/stream/{aggregate.rs,codec.rs,taps.rs}` (new), `src-tauri/src/proxy/stream/mod.rs`, `src-tauri/src/proxy/handlers.rs`, `src-tauri/src/proxy/response_processor.rs`, `src-tauri/src/proxy/handler_config.rs`, `src-tauri/src/proxy/usage/parser.rs`.

**Commits.** `390cbde` (code and tests); the follow-up commit records these notes.

**Tests.**
- `stream::taps::tests::usage_tap_memory_is_constant_in_event_count` pushes 100k events for each format. A new model string goes in on every event, yet the bytes the tap retains after 100k events equal the bytes after 100. The old collector kept every filtered event.
- `codex_first_completed_event_wins_over_later_chat_usage`, `codex_chunk_model_after_completed_is_still_a_fallback`, `openai_last_usage_chunk_wins_even_if_unparseable` and `timing_tap_records_first_byte_and_first_token_once`.
- `response_processor::tests::logged_passthrough_folds_usage_through_taps` covers 1000 filtered-out deltas, a `message_delta` split across chunks with no trailing blank line, bytes forwarded unchanged and the callback firing once. This test fails on the old `take_sse_block` tap, which dropped the trailing event.
- `stream::codec::tests` checks that each trait wrapper's output is byte-identical to calling the converter directly.
- `stream::aggregate::tests` holds the 44 aggregator and diagnostic tests moved from handlers, plus `parse_upstream_json` and Anthropic aggregator tests.
- The existing `handler_config` model-extractor tests and `usage::parser` stream tests now run through `UsageTap`.

**Verification.**
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: clean.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: clean. Also clean with `--features testkit --tests -- -D warnings`.
- `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden --test proxy_e2e`: 6 + 22 passed. The golden snapshots are unchanged: no `.snap.new` files and no fixture diffs.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib proxy`: 1334 passed.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: 2494 passed, 0 failed (lib 2328 passed, 3 ignored).
- `wc -l src-tauri/src/proxy/handlers.rs`: 865.
