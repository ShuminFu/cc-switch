# CCS-178: [proxy] Migrate the Responses bridges to codecs and remove the legacy engine

**Status:** Partial · **Kind:** refactor · **Labels:** `refactor`, `phase:P5`, `p2-medium`, `area:proxy`

> **Ticket key:** `p5-codec-responses` · **Phase:** P5 · **Priority:** p2-medium · **Effort:** XL · **Depends on:** `p5-codec-gemini`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
- Migrate responses<->anthropic, then responses->chat.
- Delete the remaining transform*.rs and streaming*.rs files and the legacy flag after one release.

## Acceptance criteria / verification
- [ ] All 5 bridges run on codecs; the old files are deleted; net reduction of at least 8k lines (all 5 bridges run on codecs with `codecEngine = ir`; deleting the legacy files waits for the one-release gate, see below)
- [ ] cached_tokens/cache_creation_input_tokens appear only in codec and usage modules (grep) (holds once the legacy files are deleted, see below)
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden --test proxy_e2e`

## Dependencies
p5-codec-gemini

## Implementation notes

**Summary.** With `codecEngine = ir`, the three remaining bridges now run through the IR codecs:
- Anthropic client ← OpenAI Responses upstream
- Codex (Responses) client ← Anthropic upstream
- Codex (Responses) client ← OpenAI Chat upstream

All five bridges are migrated. The default stays `legacy`, as in CCS-176/177. The ticket asks for the legacy `transform*.rs` / `streaming*.rs` files and the flag to be deleted only after one release. That release has not shipped, so the deletion is still to do (status **Partial**). Everything the codecs share with the legacy files has been moved into `codec/`, so the deletion is now a pure removal.

**Bridge 3 – Anthropic ← Responses** (`codec/responses/{request,response,stream}.rs`):
- Request encode follows the old `anthropic_to_responses`:
  - instructions without the billing header
  - tool calls and results promoted to `function_call` / `function_call_output`
  - bridge reasoning envelopes replayed
  - a reasoning item with no follower is dropped
  - Codex OAuth constraints
- Response decode: failed, cancelled and error-envelope bodies are errors. Strict argument parsing on `completed`. Reasoning items go into `ccswitch-openai-reasoning-v1` signatures.
- `ResponsesEventDecoder` is a state-machine port of `streaming_responses`.
- `bridge::responses_to_anthropic_sse` keeps the whole-JSON-body fallback through the new `transcode_sse_or_json_stream`. A final event without a trailing blank line is still processed (`TranscodeOptions::flush_final_event`).

**Bridge 4 – Codex ← Anthropic:**
- Anthropic upstream side:
  - `codec/anthropic/upstream_request.rs`: history sanitising (incomplete tool turns, leading user, trailing prefill), the thinking budget / adaptive / forced-tool rules, and `disable_parallel_tool_use`
  - `upstream_response.rs`
  - `upstream_stream.rs`, i.e. `AnthropicEventDecoder` and `message_events` for whole-JSON bodies
- Responses client side:
  - `codec/responses/client_request.rs`
  - `client_response.rs`
  - `client_stream.rs`, i.e. `ResponsesEventEncoder`
- The two legacy Codex converters lay out items differently. `ResponsesFlavor { AnthropicUpstream, ChatUpstream }` keeps each one's shape during the migration: item ids, unsigned-thinking handling, `Read` sanitising, tool-item `reasoning_content`, the custom-tool input delta and the usage field order.

**Bridge 5 – Codex ← Chat:**
- `codec/responses/client_request/chat.rs` ports the legacy reasoning-attachment algorithm:
  - pending reasoning goes forward to the next assistant or tool-call message, and back to the previous assistant at a user boundary or at the end of input
  - unique merge with item `reasoning_content`
  - tool outputs pre-rendered as Chat tool text
- `codec/openai_chat/request_responses.rs` is the Codex Chat request encoder:
  - system collapse to the head
  - per-message `reasoning_content` and the `tool call` placeholder
  - provider `CodexChatReasoningConfig` (thinking / effort params and value modes)
  - passthrough fields
  - the no-tools cleanup and `include_usage`
- `openai_chat/response.rs::decode_response_for_responses` and `OpenAiChatResponsesDecoder` handle `<think>` splitting, ordered tool starts, late starts and truncation.

**IR additions:**
- `Part::ToolResult.plain`: a string result vs a block list
- `ToolDef.strict`
- `MediaSource::File`: OpenAI `file_id` / `file_data`
- `MessageStart.created` / `ChatResponse.created`
- `StreamEvent::UpstreamTruncated`: the following `MessageStop` was synthesised after a truncated upstream
- `AnthropicEventEncoder::with_tool_start_input()`

**Moved to `codec/`**, re-exported from the legacy files so callers are unchanged, together with their unit tests:
- the Codex tool context (`responses/tools.rs`)
- the Anthropic thinking envelope (`responses/reasoning.rs`)
- the Codex effort→thinking tables (`anthropic/thinking.rs`)
- the Anthropic SSE aggregation fallback (`anthropic/sse_aggregate.rs`)
- the OpenAI-compatible helpers (`openai_chat/support.rs`)
- `chat_error_to_response_error` (`responses/errors.rs`)
- `build_anthropic_usage_from_responses` (`responses/usage.rs`)
- `extract_gemini_model` (`gemini/mod.rs`)
- `TOOL_RESULT_ERROR_MARKER` and the `Read` sanitisers (`responses/mod.rs`)

**Wiring:**
- `claude.rs`, `handlers.rs` (both non-stream paths), `pipeline/request_builder.rs` (both Codex encoders) and `stream/codec.rs` (`ResponsesToAnthropic`, `ChatToResponses`, `AnthropicToResponses` as decoder and encoder) call the new `codec::bridge` entry points with `settings::codec_engine()`.
- `stream/aggregate.rs` uses the moved aggregator.
- The testkit runs every bridge through `codec::bridge`.

**Parity checker.** `tests/proxy_golden.rs` adds `golden_*_ir` and `codec_parity_*` for `anthropic__responses`, `responses__anthropic` and `responses__chat`. Every fixture gives the same upstream request, raw client stream bytes, client response, usage and check failures under both engines. Result: 0 mismatches, and no fixture or snapshot changed.

Differential tests on hand-written inputs beyond the corpus:
- `codec::responses::tests`, for bridge 3: request, response and stream branches, including whole-JSON bodies and every byte split
- `codec::responses::client_anthropic_tests`, for bridge 4: the request thinking matrix (models × efforts × max tokens × histories × tool choices), response and JSON-body frames, tools, truncation, errors and framing
- `codec::responses::client_chat_tests`, for bridge 5: reasoning attachment, media, tools and passthrough, the reasoning-config matrix, response variants, think tags across chunks, parallel and late tool calls, errors and truncation

**Intentional IR-engine differences.** All are on malformed, inconsistent or rare inputs, and each is asserted in the `ir_engine_*` tests:
- **Same rules as CCS-176/177:**
  - An integral float is written as an integer.
  - `stream: false` is not forwarded.
  - `role: system` messages fold into the system prompt.
  - String `tool_choice: "any"` becomes `required`.
  - A message or tool result with no content (or `null` content) is dropped or left empty.
  - Nothing is emitted after a terminal event, including a transport error after `message_stop` / `response.completed`, or data after `[DONE]`.
  - SSE framing follows WHATWG.
- **Responses / Anthropic usage:**
  - The Anthropic `cache_creation` TTL breakdown is no longer passed through.
  - For the Chat flavour, the upstream `total_tokens`, extra `completion_tokens_details` fields and the top-level `cache_read_input_tokens` alias are not copied. Token values are unchanged.
- **Ids:** An empty Anthropic `message.id` gives `resp_ccswitch` on the stream path too. The legacy stream path gave `resp_`.
- **Chat flavour:**
  - Refusal content becomes `output_text`, because the IR has no refusal part.
  - `input_audio` parts are dropped.
  - A system or developer message with non-text parts is folded into the system prompt as text.
  - The nested Chat-style `{"function": {...}}` tool definition is rebuilt in canonical field order.
  - A tool's `reasoning_content` is taken when the tool starts, not when its first fragment arrives.
- **Thinking envelopes** are rebuilt from the IR as type/thinking/signature, so unusual key orders or extra fields in the upstream thinking block are not kept byte for byte.

**Remaining (blocked on the release gate):**
1. After one release with `codecEngine = ir` and no regressions:
   - Delete `providers/transform{,_responses,_codex_anthropic,_codex_chat,_gemini}.rs` and `providers/streaming{,_responses,_codex_anthropic,_codex_chat,_gemini}.rs`, 18,397 lines today, legacy tests included.
   - Delete the `CodecEngine` flag and its `codecEngine` setting.
   - Point the few callers that use the re-exports (`handlers.rs`, `errors/render.rs`, `pipeline/endpoint.rs`, `stream/codec.rs`, `claude.rs`, `testkit.rs`) at `codec::*`.
   - Remove the legacy arms of `codec::bridge` and the parity tests. The `golden_*_ir` runs become the only golden runs.
2. This branch adds about 9.4k lines under `codec/`, about 2.5k of them tests. Measured against the 18.4k lines to delete, the net reduction is about 9k lines, so the at-least-8k target is reachable but can only be confirmed after the deletion.
3. The grep criterion holds after the deletion. Outside the legacy files, the only matches outside `codec/` and `usage/` today are:
   - `proxy/stream/taps.rs`, the usage tap
   - `services/session_usage.rs`, which reads local Claude Code session logs

   Both are usage accounting, not protocol conversion.

   `proxy/providers/testkit.rs` also matches, in a hand-written Responses `usage` fixture (`input_tokens_details.cached_tokens`) for the golden checks. It is test data, not conversion code, but it stays after the deletion unless the grep is limited to non-test code.

**Files:**
- Codec, new:
  - `src-tauri/src/proxy/codec/responses/{mod,request,response,stream,client_request,client_response,client_stream,reasoning,tools,errors,usage,tests,client_anthropic_tests,client_chat_tests}.rs`
  - `src-tauri/src/proxy/codec/responses/client_request/chat.rs`
  - `src-tauri/src/proxy/codec/anthropic/{upstream_request,upstream_response,upstream_stream,thinking,sse_aggregate}.rs`
  - `src-tauri/src/proxy/codec/openai_chat/{request_responses,stream_responses,support}.rs`
- Codec, changed:
  - `src-tauri/src/proxy/codec/{mod,bridge,transcode,tests}.rs`
  - `src-tauri/src/proxy/codec/anthropic/{mod,request,stream,tests}.rs`
  - `src-tauri/src/proxy/codec/openai_chat/{mod,request,response,stream,tests}.rs`
  - `src-tauri/src/proxy/codec/gemini/{mod,request,stream,tests}.rs`
  - `src-tauri/src/proxy/protocol/ir.rs`
- Callers:
  - `src-tauri/src/proxy/{handlers.rs,pipeline/request_builder.rs,stream/codec.rs,stream/aggregate.rs}`
  - `src-tauri/src/proxy/providers/{claude,testkit}.rs`
- Legacy re-exports only:
  - `src-tauri/src/proxy/providers/{transform,transform_responses,transform_codex_anthropic,transform_codex_chat,transform_gemini}.rs`
- Tests:
  - `src-tauri/tests/proxy_golden.rs`

**Commits:**
- 492e97f: bridge 3
- 6f1a020: bridge 4
- 392d2dc: bridge 5
- cc7df39: shared helpers moved into codec
- the follow-up commit with this note

**Verification:**
- `cargo fmt --check`: clean
- `cargo clippy --all-targets -- -D warnings`: clean
- `cargo test --lib codec::`: 123 passed (51 in `codec::responses`, including the 3 new differential suites and the moved helper tests)
- `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden --test proxy_e2e`: 16 + 27 passed. That adds `golden_{anthropic_responses,responses_anthropic,responses_chat}_ir` and `codec_parity_{anthropic_responses,responses_anthropic,responses_chat}`.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: 2855 passed, 0 failed
- No frontend changes.

**Review.** I went through the five commits independently and checked:
- **Wiring.** Every production call of the legacy request, response and SSE converters now goes through `codec::bridge` with `settings::codec_engine()`. The remaining direct uses of `providers::transform*` are only the re-exported helpers and legacy unit tests.
- **Error kinds.** Codex request errors still map to `InvalidRequest`, and response errors to `TransformError`, as before.
- **Probe beyond the corpus.** A throwaway differential probe (not committed) fed realistic payloads through both engines and got byte-identical output:
  - Codex requests with developer, image and file inputs, reasoning, `function_call`, `custom_tool_call`, `local_shell_call` and `web_search_call` items, a string `input`, a forced tool and several efforts, through both Codex bridges
  - Claude Code requests with a billing header, `cache_control`, thinking, adaptive thinking or `output_config`, and tool results that carry images and errors
  - streams and non-stream bodies for all three bridges, including redacted thinking, server tools, an error envelope, `max_tokens` / `refusal` stops, `<think>` splitting and incomplete Responses bodies

  No defects found. Verification results:
  - `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` are clean.
  - `cargo test --no-fail-fast` passes everything: lib 2632, `proxy_golden` 16, `proxy_e2e` 27, 0 failed.

  The only change is the `testkit.rs` grep note above. The status stays **Partial**: deleting the legacy files and the 8k-line net reduction are still gated on the one-release soak.

### Remaining after triage

- Blocked: Delete the 10 legacy files (18,397 lines), the CodecEngine flag and the codec_parity tests. Time-gated: one shipped release with codecEngine = ir and no regressions.
- Blocked: Confirm the ≥ 8k net reduction. Needs a maintainer decision; it measures about -2.8k.
- Blocked: Grep criterion fully met. Needs the gated deletion, plus a maintainer decision on whether taps.rs, session_usage.rs and testkit.rs count as usage/test modules (or CCS-244).
- Blocked: Shipping codecEngine = ir as the default. Needs a maintainer release decision (code in CCS-246).
- Follow-up [CCS-243](../P5-protocol-ir-commands/CCS-243-p5-usage-token-name-check.md): Add a check script that keeps cache-token field names inside codec/ and usage/.
- Follow-up [CCS-244](../P5-protocol-ir-commands/CCS-244-p5-usage-token-names-out-of-taps.md): Move cache-token field parsing out of stream taps, session_usage and the provider testkit.
- Follow-up [CCS-245](../P5-protocol-ir-commands/CCS-245-p5-proxy-repoint-legacy-reexports.md): Repoint non-legacy callers from legacy transform re-exports to codec::*.
- Follow-up [CCS-246](../P5-protocol-ir-commands/CCS-246-p5-codec-engine-ir-default.md): Prepare the default flip to codecEngine = ir (merge needs a maintainer release decision).

## Owner decision: GitHub Actions checks skipped

By the repository owner's decision on 2026-10-31, checks that need a GitHub Actions run are skipped (not verified). None of this ticket's remaining items depend only on GitHub Actions, so that decision does not change its status.

### Still blocked by

- Delete the 10 legacy transform*/streaming*.rs files (18,397 lines), the CodecEngine flag and codecEngine setting, the legacy bridge arms and the codec_parity tests. **Needs:** Release soak: one shipped release with codecEngine = ir and no regressions
- Net reduction of at least 8k lines (AC1). **Needs:** The release-gated deletion first, plus a maintainer decision because triage measures about -2.8k against the >= 8k target
- Grep criterion: cached_tokens/cache_creation_input_tokens only in codec and usage modules (AC2). **Needs:** The release-gated deletion, plus a maintainer decision on whether stream/taps.rs, services/session_usage.rs and providers/testkit.rs count as usage/test modules (or CCS-244 is done)
- Ship codecEngine = ir as the default (prerequisite for the soak, code in CCS-246). **Needs:** Maintainer release decision
