# Proxy format matrix

How cc-switch's local proxy maps fields between the four wire protocols it
bridges: Anthropic Messages, OpenAI Chat Completions, OpenAI Responses and
Gemini `generateContent` (PROV-31).

Responses are converted through one canonical IR
(`src-tauri/src/proxy/providers/ir/`): every upstream response is decoded once
into IR parts, stop reason and usage, then encoded once for the client protocol.
Requests still use the per-bridge transforms listed below.

Legend: **mapped** (converted to the target's equivalent), **dropped** (removed
by design, the target has no equivalent), **error** (the request is rejected
with a transform error instead of being silently changed).

## Anthropic request → upstream

| Anthropic field | OpenAI Chat (`openai_chat`) | OpenAI Responses (`openai_responses`) | Gemini (`gemini_native`) |
|---|---|---|---|
| `model` | mapped | mapped | mapped to the URL model |
| `system` | mapped (system message) | mapped (`instructions`) | mapped (`systemInstruction`) |
| `max_tokens` | mapped (`max_tokens`, or `max_completion_tokens` for gpt-5 / o-series) | mapped (`max_output_tokens`) | mapped (`generationConfig.maxOutputTokens`) |
| `temperature`, `top_p` | mapped (dropped for gpt-5 / o-series, which reject custom sampling) | mapped | mapped (`generationConfig`) |
| `top_k` | dropped | dropped | mapped (`generationConfig.topK`) |
| `stop_sequences` | mapped (`stop`) | dropped (not supported by Responses) | mapped (`generationConfig.stopSequences`) |
| `stream` | mapped (plus `stream_options.include_usage`) | mapped | selects `streamGenerateContent` |
| `thinking`, `output_config.effort` | mapped (`reasoning_effort`, or the provider's vendor reasoning form) | mapped (`reasoning.effort`, `reasoning.summary: "auto"`) | mapped (`generationConfig.thinkingConfig`) |
| `tools` | mapped (`function` tools; `BatchTool` dropped; schema sanitized) | mapped (same as Chat) | mapped (`functionDeclarations`; schema sanitized) |
| `tool_choice` | mapped (`any` becomes `required`; `disable_parallel_tool_use` becomes `parallel_tool_calls: false`) | mapped (same as Chat) | mapped (`functionCallingConfig`); unknown values are an error |
| `metadata`, `service_tier`, `container`, `mcp_servers` | dropped | dropped (Codex OAuth fast mode sets `service_tier` itself) | dropped |

### Content blocks

| Anthropic block | Chat | Responses | Gemini |
|---|---|---|---|
| `text` | mapped | mapped (`input_text` / `output_text`) | mapped (`text` part) |
| `image` (base64) | mapped (`image_url` data URL) | mapped (`input_image`) | mapped (`inlineData`) |
| `image` (url) | mapped (`image_url` with the http(s) URL) | mapped | error: Gemini Native only accepts base64 media |
| `document` (base64 PDF) | mapped (`file` part) | mapped (`input_file`) | mapped (`inlineData`) |
| `tool_use` | mapped (`tool_calls`) | mapped (`function_call` item) | mapped (`functionCall`); synthesized ids are not forwarded |
| `tool_result` | mapped (`role: tool` message; media moved to a follow-up user message; `is_error` prefixes `[error]`) | mapped (`function_call_output`; `is_error` adds a marker) | mapped (`functionResponse`; `is_error` uses the `error` key; returned images/PDFs follow as `inlineData` parts) |
| `thinking` / `redacted_thinking` | replayed as `reasoning_content` for providers that need it, otherwise dropped | the cc-switch reasoning envelope is restored as the original `reasoning` item; other thinking is dropped | dropped; a `ccswitch-gemini-thought-v1` envelope restores `thoughtSignature`s |

## Upstream response → client

These rows come from the IR codecs, so streaming and non-streaming responses
follow the same rules.

| Concern | Rule |
|---|---|
| Usage | IR `input` is always fresh (cache-exclusive) input. Chat, Responses and Gemini decoders subtract cache reads and writes from their inclusive input count; the Responses encoder adds them back. `input + cache_read + cache_write` equals the upstream's input count on every bridge. |
| Stop reason | `length` / `MAX_TOKENS` / `incomplete(max_output_tokens)` map to `max_tokens`; content filters and Gemini safety reasons map to `refusal` (Responses: `incomplete` with `content_filter`); tool calls without an explicit reason map to `tool_use`. |
| Reasoning | Chat `reasoning_content` / `reasoning` / `reasoning_details` and Gemini `thought` parts become `thinking`; Responses `reasoning` items keep their encrypted state in a `ccswitch-openai-reasoning-v1` envelope; Anthropic signed thinking keeps its state in a `ccswitch-anthropic-thinking-v1` envelope towards Responses clients. |
| Refusals | Chat and Responses `refusal` content becomes Anthropic `text` (Responses clients keep a `refusal` part). |
| Tool ids | A missing Chat tool-call id becomes `call_<index>`; a missing Gemini id is synthesized and never forwarded upstream. |
| Unknown Anthropic blocks | Passed through unchanged to Anthropic clients and dropped for Responses clients. |
