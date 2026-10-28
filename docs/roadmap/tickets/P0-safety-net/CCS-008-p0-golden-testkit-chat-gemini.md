# CCS-008: [proxy-tests] Add golden bridge testkit with chunk-split fuzzing, plus Chat and Gemini fixtures

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p1-high`, `area:proxy-tests`

> **Ticket key:** `p0-golden-testkit-chat-gemini` · **Phase:** P0 · **Priority:** p1-high · **Effort:** M
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Most recent fix commits (6d316c0, 27ce0a5, 650905a, 613fef7, f991726) are in the protocol bridges. Bridge behaviour is pinned only by ad-hoc inline concat!() tests, so behaviour cannot be compared before and after a refactor, and chunk-boundary bugs go unseen.

## Evidence
- Inline cases in src-tauri/src/proxy/providers/streaming.rs and streaming_gemini.rs
- Gemini finishReason mapping drift: streaming_gemini.rs:18 vs transform_gemini.rs:1142

## Proposed change
- Add a `proxy::providers::testkit` module under `cfg(any(test, feature="testkit"))`, plus src-tauri/tests/proxy_golden.rs.
- Fixture layout: tests/fixtures/proxy/<bridge>/<case>/{client_request.json, upstream_request.expected, upstream_response.sse|json, client_response.expected, usage.expected, meta.toml (intent and source commit)}.
- Snapshot with insta, redacting ids and timestamps.
- Re-split every stream input at every byte offset (inputs of 4 KB or less), at 200 random splits, and at UTF-8 and CRLF boundaries.
- Assert structural invariants separately from the snapshots: start/stop pairing, stable tool ids, Chat index order, reasoning before message, exactly one terminal event, usage conservation.
- Port the existing cases for anthropic__openai_chat and anthropic__gemini. Record the Gemini finishReason drift as an expected-diff fixture.

## Acceptance criteria / verification
- [x] At least 30 fixtures across anthropic__openai_chat and anthropic__gemini, covering stream and non-stream
- [x] Invariant failures report the split offset
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden` passes
- [x] `cargo test --manifest-path src-tauri/Cargo.toml proxy::` still passes (the ~1046 inline tests)
- [x] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`

## Dependencies
None.

## Implementation notes

**Summary**
- New `proxy::providers::testkit` (`cfg(any(test, feature = "testkit"))`, re-exported as `cc_switch_lib::proxy_testkit`, `#[doc(hidden)]`). The new `testkit` Cargo feature is turned on for integration tests by a self dev-dependency (`cc-switch = { path = ".", features = ["testkit"] }`), so `cargo test --test proxy_golden` needs no extra flags. Release builds do not compile the testkit. `insta` is a new dev-dependency.
- Fixture layout: `src-tauri/tests/fixtures/proxy/<bridge>/<case>/` with `meta.toml` (intent, `source_commit`, optional `ported_from`, `[options]`, `[invariants]`, `[parity]`), optional `client_request.json`, `upstream_response.sse` and/or `upstream_response.json`. The expected outputs are insta snapshots in the same directory: `upstream_request.snap`, `client_response.snap` and `usage.snap`. They play the role of the `*.expected` files in the proposal, using insta's file format. Generated ids, meaning ids that appear in none of the fixture inputs (`gemini_synth_*`, `tool_call_<n>`, ...), become `[generated-id:N]`, numbered by first appearance, so equal ids stay equal. Ids copied from the inputs are kept so the snapshots still show pass-through. Timestamps become `[timestamp]`.
- The driver mirrors the proxy: Chat requests go through `anthropic_to_openai_with_reasoning_content` + `inject_openai_stream_include_usage`, and responses through `create_anthropic_sse_stream` / `openai_to_anthropic`. Gemini requests go through `anthropic_to_gemini`, and responses through `create_anthropic_sse_stream_from_gemini` / `gemini_to_anthropic_with_shadow_and_hints`, with tool schema hints taken from the client request. Usage is what `TokenUsage::from_claude_stream_events` / `from_claude_response` records.
- Chunk-split fuzzing: every stream input is split at every byte offset (inputs of 4 KB or less; larger inputs get every UTF-8 continuation byte and CRLF offset). On top of that come all UTF-8 boundaries at once, all CRLF boundaries at once, one byte per chunk, and 200 random multi-splits seeded from the fixture id (SplitMix64, reproducible). Each split must give the same redacted event sequence as the unsplit input, and must pass the invariants. Failures read like `<bridge>/<case>: split byte-offset at offsets [75]: output differs from unsplit input: event #2: ...`, or `split random#17 seed=0x... at offsets [...]: invariant violated: [terminal] ...`.
- Invariants, checked separately from the snapshots: `event_names` (SSE event name == payload type), `message_start` (at most one, first), `block_pairing` (start/delta/stop pairing, all closed before message_delta/stop), `index_order` (Chat/Gemini block indices 0,1,2,... in emission order), `tool_ids` (non-empty, unique id + name), `tool_input_json`, `reasoning_first`, `terminal` (exactly one of message_stop / error / stream `Err` as last event, per `invariants.terminal`; at most one message_delta), `usage_conservation` (client input + cache_read + cache_creation == upstream prompt; output and cache_read equal; usage parsed in the upstream's native format). Non-stream responses get the subset that applies. A fixture can skip an invariant only with `skip_reason`.
- Parity: a fixture that has both `.sse` and `.json` compares the folded stream message against the non-stream response on content (ids ignored), stop_reason and token counts. It asserts that the diff equals `[parity].expected_diff`.
- 58 fixtures. `anthropic__openai_chat`: 27 (17 stream, 8 non-stream, 7 with a client request). `anthropic__gemini`: 31 (23 stream, 9 non-stream, 7 with a client request, 3 parity). They port the inline cases in `streaming.rs`, `streaming_gemini.rs`, `transform.rs` and `transform_gemini.rs` (`ported_from` names each source test), and add CRLF, 4-byte emoji, reasoning→text, text→tool, length/MAX_TOKENS/SAFETY/blocked-prompt, cached usage, upstream error and `[DONE]` cases.
- Gemini finishReason drift (`streaming_gemini.rs:18` vs `transform_gemini.rs:1142`): recorded in `anthropic__gemini/drift_finish_reason_unknown_with_tool_call` with `expected_diff = ['stop_reason: stream="tool_use" non_stream="end_turn"']`, for an unknown finishReason with a tool call. Fixing the drift makes this fixture fail until `expected_diff` is updated. `drift_interleaved_text_and_tool_order` records the documented part-order trade-off in the same way. `parity_text_stop` pins a case with no diff.
- Finding (not fixed here, out of scope): `openai_to_anthropic` turns the legacy `function_call` field into a `tool_use` with an empty `id`. `nonstream_legacy_function_call` skips `tool_ids` and gives that as the `skip_reason`. A follow-up ticket should decide on a synthesized id.
- `tests/fixtures/proxy/.gitattributes` marks `*.sse` as `-text`, because the repo-wide `* text=auto` would otherwise normalize CRLF fixtures to LF. `*.snap` is pinned to LF. `golden_fixture_inventory` fails if a `*crlf*` fixture loses its CRLF. A nested `.gitignore` ignores pending `*.snap.new` files.
- Updating snapshots after an intended behaviour change: `INSTA_UPDATE=always cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden` (or `cargo insta review`), then review the diff.

**Files**
`src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `src-tauri/src/lib.rs`, `src-tauri/src/proxy/providers/mod.rs`, `src-tauri/src/proxy/providers/testkit.rs` (new), `src-tauri/tests/proxy_golden.rs` (new), `src-tauri/tests/fixtures/proxy/**` (new: 58 cases, `.gitattributes`, `.gitignore`).

**Commits**
- `9601f02` test(proxy): add golden bridge testkit with chunk-split fuzzing, Chat and Gemini fixtures (CCS-008)

**Verification (Linux)**
- `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden`: 3 passed (`golden_anthropic_openai_chat`, `golden_anthropic_gemini`, `golden_fixture_inventory`), about 5 s.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib proxy::`: 1142 passed, 0 failed (this includes the 8 new `proxy::providers::testkit::tests` self-tests). `cargo test --lib`: 2072 passed, 0 failed, 2 ignored. Full `cargo test --tests`: every binary green.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`: clean. `cargo clippy ... -- -D warnings` (without the testkit): clean. `cargo fmt --check`: clean. `cargo deny check licenses bans sources`: ok.
- Proof that the fuzzer works (fails before, passes after): I temporarily replaced `append_utf8_safe` in `streaming.rs` with per-chunk `String::from_utf8_lossy` (the bug the UTF-8 fix removed). `proxy_golden` then failed with 18 reported failures, for example `anthropic__openai_chat/stream_cjk_text: split byte-offset at offsets [75]: output differs from unsplit input: event #2: expected ..."你好"... got ..."���好"...`. Restoring the file makes it pass again. The self-tests `fuzzer_reports_split_offset_of_chunk_boundary_bug` and `fuzzer_reports_split_offset_of_invariant_violation` pin the offset reporting.
