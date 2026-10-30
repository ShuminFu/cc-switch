# CCS-187: [proxy] Add the first new client/upstream combination: Gemini CLI to an Anthropic upstream

**Status:** Done · **Kind:** feature · **Labels:** `feature`, `phase:P5`, `p3-low`, `area:proxy`

> **Ticket key:** `p5-gemini-cli-anthropic` · **Phase:** P5 · **Priority:** p3-low · **Effort:** M · **Depends on:** `p5-codec-gemini`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
Add an endpoint plus a RoutePlan for a Gemini client to an Anthropic upstream, using the codecs.

## Acceptance criteria / verification
- [x] At least 10 new golden fixtures, stream and non-stream
- [x] proxy_e2e scenario
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden`

## Dependencies
p5-codec-gemini

## Implementation notes

**Summary.** Gemini CLI (and other Gemini Native clients) can now use an Anthropic Messages upstream. A Gemini provider opts in with `meta.apiFormat = "anthropic"` (or `settingsConfig.apiFormat` / `api_format`; the same values as Codex: `anthropic` / `claude` / `messages`). Nothing is inferred from the base URL. The provider keeps its Gemini settings: `GOOGLE_GEMINI_BASE_URL` points at the Anthropic endpoint, `GEMINI_API_KEY` holds the Anthropic key, and the model comes from the client URL (`GEMINI_MODEL`, e.g. `claude-sonnet-4-5`). Provider model mapping applies as usual.

- **Endpoint and route plan.** The existing `/v1beta/*`, `/gemini/v1beta/*` and `/gemini/v1/*` mounts are unchanged. `RouteFlags::gemini_to_anthropic` is set for `models/{model}:generateContent` and `:streamGenerateContent` on such a provider (`GeminiGenerateEndpoint::parse`). The upstream endpoint becomes `/v1/messages`. The Gemini query (`alt=sse`, `key=`) is dropped, and the full-URL guard for a pasted `/v1/messages` base is reused. The plan's upstream is `AnthropicMessages`, and `ResponseDecode::GeminiFromAnthropic` drives `handle_gemini_anthropic_transform`. Header policy, the 2xx Anthropic envelope check and identity encoding follow the Codex→Anthropic path (`RouteFlags::foreign_client_to_anthropic`). Auth is `x-api-key`, or `Authorization: Bearer` when `meta.apiKeyField = ANTHROPIC_AUTH_TOKEN`.
- **Codec** (`codec/gemini/client_{request,response,stream}.rs`, `GeminiClientCodec`, `GeminiEventEncoder`). This combination has no legacy converter, so it always runs on IR and ignores `codecEngine`.
  - **Request:** `systemInstruction`, `contents`, `functionDeclarations` and `toolConfig` are decoded:
    - Gemini `Schema` becomes JSON Schema (lower-case types, `nullable` folded into the type). `parametersJsonSchema` passes through. Built-in tools are dropped.
    - `ANY` with exactly one allowed name forces that tool.
    - `generationConfig` maps temperature, topK, maxOutputTokens and stopSequences. topP is kept only when temperature is absent, because newer Claude models reject both together and Gemini CLI sends both.
    - `thinkingConfig`: budget 0 disables thinking, a positive budget is passed as-is, `thinkingLevel` becomes the effort, and -1 (dynamic) leaves the model default.
    - `responseMimeType: application/json` adds a JSON-only system instruction that includes the schema.
    - `functionCall` parts without ids get deterministic positional ids. `functionResponse` parts are paired by id, then by name, then by order. `{output}` and `{error}` map to tool_result and `is_error`. Ids are sanitized to `[A-Za-z0-9_-]`.
    - Unsigned `thought` parts are dropped. Consecutive same-role contents are merged, and tool results move to the front of the user message.
    - inlineData and fileData map to image or document blocks, or to a text note for media the upstream cannot read.
  - **Response and stream:**
    - Text and visible thinking become parts, and tool calls become `functionCall` with the Anthropic id (Gemini CLI echoes it back).
    - Signed thinking and redacted_thinking are packed into the first `functionCall.thoughtSignature` (prefix `ccswitch-anthropic-thinking-list-v1:`), and request decode restores them. This lets tool-result continuations keep thinking on, and lets models that cannot disable thinking keep working.
    - The stream emits text and thought deltas live and each tool call whole when its block stops. `finishReason` and the final `usageMetadata` ride on the last content frame. Errors become `{"error":{code,message,status}}` frames.
    - A non-stream Anthropic body returned to a stream request is converted whole.
  - **Upstream errors:** Anthropic error envelopes from upstream errors are re-rendered in Gemini shape with the HTTP status kept (`errors/render.rs`).
  - **Usage:** recorded as `app_type = gemini`. `promptTokenCount` includes cache reads and writes, and `cachedContentTokenCount` carries cache reads.
- **Anthropic upstream encoder** (shared with Codex): it now forwards `stop_sequences` and `top_k` (sampling only), and prefers an explicit `reasoning.budget_tokens` over the effort-derived budget. The Codex decoder never sets these fields, so all existing goldens are byte-identical.
- **Golden testkit:** adds the `gemini__anthropic` bridge with Gemini client invariants (data-only frames, one terminal frame, functionCall id/args, thought-first, usage conservation) and stream/non-stream parity on merged parts. It also adds options `client_model` and `client_stream`, because Gemini carries both in the URL. Bug-report capture export maps to the new bridge and fills those options from the captured URL.

**Known limitations** (documented, not blocking):
- Gemini has no cache-write usage field. Anthropic `cache_creation_input_tokens` is folded into `promptTokenCount` and billed as fresh input. Gemini clients send no `cache_control` and no cache breakpoints are injected on this path, so this only occurs with gateways that cache on their own.
- `:countTokens` and `GET models` are not converted for an Anthropic-format Gemini provider. They pass through and the Anthropic upstream answers 404.
- There is no UI control yet. The upstream format is set through the provider JSON (`meta.apiFormat`).

**Files:**
- `src-tauri/src/proxy/codec/gemini/{client_request,client_response,client_stream,client_tests}.rs` (new)
- `src-tauri/src/proxy/codec/gemini/mod.rs`, `codec/{mod,bridge}.rs`, `codec/anthropic/upstream_request.rs`
- `src-tauri/src/proxy/{routing/plan,pipeline/request_builder,pipeline/endpoint,forwarder,handlers,stream/codec,errors/render}.rs`
- `src-tauri/src/proxy/providers/{gemini,mod,testkit}.rs`
- `src-tauri/src/proxy/capture/{mod,export,tests}.rs`
- `src-tauri/src/proxy_harness.rs` (Gemini providers and a `gemini()` client helper)
- `src-tauri/tests/proxy_e2e.rs`, `src-tauri/tests/proxy_golden.rs`
- `src-tauri/tests/fixtures/proxy/gemini__anthropic/` (17 fixtures: 4 non-stream, 2 stream/non-stream parity, 8 stream, 3 request-only)

**Commits:**
- 0ddfcb2: codec, routing, handler and proxy_e2e.
- The follow-up commit with the golden bridge, fixtures and capture export, which cite 0ddfcb2 as `source_commit`, plus this note.

**Verification:**
- `cargo fmt --check`: clean
- `cargo clippy -- -D warnings` and `cargo clippy --all-targets -- -D warnings`: clean
- `cargo test --lib gemini::client_tests`: 16 passed
- `cargo test --lib providers::gemini`: 29 passed
- `cargo test --lib`: 2670 passed, 2 ignored
- `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden`: 18 passed. This adds `golden_gemini_anthropic`, which requires at least 10 fixtures with at least 5 stream and 5 non-stream. It runs split fuzzing and invariants on every case, and `usage_parity_golden_corpus` now covers the new bridge. No existing fixture or snapshot changed.
- `cargo test --test proxy_e2e gemini`: 3 passed (non-stream, stream and 429 error scenarios). They check the outbound `/v1/messages` request, the headers, the Gemini-shaped reply and the `gemini` usage log row.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: 2900 passed, 0 failed (2898 at 0ddfcb2).
- No frontend changes, so the pnpm checks are not affected.

**Review.** Every acceptance item was checked against the code and re-run: 17 golden fixtures (stream and non-stream), three `proxy_e2e` scenarios, and `proxy_golden` passing. The full Rust suite passed with 2900 passed and 0 failed. The review fixed two gaps:
- Editing a Gemini provider in the UI wrote `undefined` to every app-specific meta field (`toGeminiAppMeta`). One save therefore erased `apiFormat`, `apiKeyField` and `maxOutputTokens`, turning the Anthropic upstream off without notice, and the Gemini auth headers then went to the Anthropic URL. The form now keeps these three fields from the existing meta (`tests/features/providerForms/gemini.test.ts`).
- Gemini accepts `temperature` in `[0, 2]`, but Anthropic accepts only `[0, 1]` and rejects other values with a 400 that is not retried. Request decode now clamps the value to `[0, 1]` (`client_tests::temperature_is_clamped_to_the_anthropic_range`).

Still open as follow-ups: the UI does not yet say that proxy takeover is required for an Anthropic-format Gemini provider, and there is still no UI control to set `apiFormat`.
