# CCS-174: [proxy] Define the canonical protocol IR and codec traits

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P5`, `p2-medium`, `area:proxy`

> **Ticket key:** `p5-ir-codec-core` · **Phase:** P5 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p3-stream-traits-taps`, `p3-stop-reason-table`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
- Add proxy/protocol/ir.rs (ChatRequest, Part, StreamEvent), reusing UsageIR and StopReason.
- Add codec traits per format with request, response and stream parts. Non-stream is stream plus aggregate.
- Add a `proxy.codecEngine=legacy` flag.

## Acceptance criteria / verification
- [x] IR unit tests; proptest for round-trip where the formats are lossless
- [x] `cargo test --manifest-path src-tauri/Cargo.toml protocol::ir`

## Dependencies
p3-stream-traits-taps, p3-stop-reason-table

## Implementation notes

**Summary.** The canonical IR and the per-format codec traits are in place, along with the `codecEngine` flag. No bridge uses them yet; the bridges migrate in CCS-176 to CCS-178. Proxy behaviour and golden fixtures are unchanged.

- `proxy/protocol/ir.rs` defines:
  - `ChatRequest`, holding system, messages, tools, tool choice, sampling params, a reasoning config, `stream` and an `extra` passthrough map
  - `Message` / `Role`
  - `Part`: text, image, document, tool call (raw JSON argument text), tool result, reasoning with an opaque signature, and redacted reasoning
  - `ChatResponse`
  - `StreamEvent` with `PartDelta`
  - It reuses `UsageIR`, emitted as a cumulative snapshot where the last one wins, and `StopReason`.
- Non-stream is stream plus aggregate:
  - `StreamAggregator` / `aggregate()` fold events into a `ChatResponse`, and `ChatResponse::to_events()` is the inverse.
  - Contract violations are reported as `AggregateError`: duplicate or unknown part, delta after stop, delta of the wrong kind, event after `message_stop`, and an upstream error.
- `proxy/codec/mod.rs` defines the traits `RequestCodec`, `ResponseCodec` and `StreamCodec`. `StreamCodec` builds a stateful `EventDecoder` / `EventEncoder` pair for each stream.
  - The umbrella `Codec` trait has a blanket impl. Its provided methods are `aggregate_stream`, for a streamed upstream going to a non-stream client, and `encode_response_as_stream`, for a non-stream upstream going to a stream client.
  - `CodecError` covers invalid input, unsupported content and aggregate errors.
- Flag: `AppSettings.codec_engine`, stored as the `codecEngine` key:
  - Values are `legacy`, the default, and `ir`.
  - Parsing is lenient: an unknown value falls back to legacy, so it never breaks settings parsing.
  - The default is not serialized.
  - `CodecEngine::uses_ir(bridge_migrated)` means bridges that have not been migrated always stay on legacy.
  - `settings::codec_engine()` is the accessor. It is wired onto the request path when the first bridge migrates (CCS-176).
  - `codecEngine?` was added to the TS `Settings` type so the frontend keeps the field.

**Files:**
- `src-tauri/src/proxy/protocol/ir.rs` (new)
- `src-tauri/src/proxy/protocol/mod.rs`
- `src-tauri/src/proxy/codec/mod.rs` (new)
- `src-tauri/src/proxy/codec/tests.rs` (new)
- `src-tauri/src/proxy/mod.rs`
- `src-tauri/src/settings.rs`
- `src/types.ts`

**Commits:** e35f99b (code), plus the follow-up commit with this note.

**Tests:**
- IR unit tests in `protocol::ir::tests`:
  - the event layout
  - aggregate as the inverse of `to_events`
  - interleaved parallel tool calls
  - usage where the last snapshot wins
  - a truncated stream
  - every contract violation
  - a signature-only delta
- proptests in `protocol::ir::tests`:
  - `response_round_trips_through_events`
  - `aggregation_is_independent_of_chunking_and_interleaving`, which splits every delta at random char boundaries and interleaves the parts
- Codec contract tests in `proxy::codec::tests` use a minimal text-only codec:
  - non-stream equals stream plus aggregate, from body to SSE through `SseDecoder` and aggregation back to the body
  - a request round trip
  - error propagation
  - lenient parsing of `codecEngine`, its serde shape inside `AppSettings`, and the `uses_ir` gating
  - the proptest `text_response_round_trips_through_sse`, covering the lossless text-only subset across both the SSE and JSON paths

**Verification:**
- `cargo fmt --check`: clean
- `cargo clippy -- -D warnings` and `cargo clippy --all-targets -- -D warnings`: clean
- `cargo test --manifest-path src-tauri/Cargo.toml protocol::ir`: 9 passed
- `cargo test --lib -- protocol::ir proxy::codec`: 16 passed
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast` (lib plus all integration tests, including `proxy_golden`): 2759 passed, 0 failed
- `pnpm typecheck`: clean
- `pnpm test:unit`: 159 files / 1547 tests passed
- `prettier --check src/types.ts`: clean

**Review.** All acceptance criteria hold: the IR unit tests and proptests pass, and `cargo test protocol::ir` passes. The flag is the top-level `codecEngine` settings key, not a nested `proxy.codecEngine`. It sits next to the other proxy toggles such as `stickySessionRouting`, and the frontend saves it through the full-settings spread, so the value is kept. Fix applied: `CodecEngine` only fell back to legacy for unknown *string* values. A non-string `codecEngine` (null, a number or an object, for example from a hand-edited settings.json) still made the whole `AppSettings` parse fail. The deserializer now reads any JSON value and falls back to legacy with a warning for anything that is not a string. A test covers this.
