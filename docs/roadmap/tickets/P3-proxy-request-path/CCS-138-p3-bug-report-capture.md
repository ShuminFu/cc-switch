# CCS-138: [proxy] Capture and export a redacted bug report in the golden-fixture layout

**Status:** Done · **Kind:** feature · **Labels:** `feature`, `phase:P3`, `p2-medium`, `area:proxy`

> **Ticket key:** `p3-bug-report-capture` · **Phase:** P3 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p3-stream-traits-taps`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Bridge bugs reported by users cannot be replayed. The critique moved this from P2 to P3 because it needs the stream taps.

## Proposed change
- Add an opt-in, time-boxed CaptureTap that records the client request, the upstream request, the upstream SSE with timings and the client SSE, all redacted.
- Add an "Export bug report" button in the request log that writes a zip matching tests/fixtures/proxy/<bridge>/<case>.

## Acceptance criteria / verification
- [x] Redaction test over a corpus of key patterns (sk-, sk-ant-, AIza, ghu_, Bearer, x-api-key, cookies): zero hits in the export
- [x] An exported capture replays green through proxy_golden
- [x] `cargo test --manifest-path src-tauri/Cargo.toml capture` and `pnpm test:unit`

## Dependencies
p3-stream-traits-taps

## Implementation notes

**Summary.** New module `src-tauri/src/proxy/capture/` adds an opt-in, time-boxed
CaptureTap. `CaptureStore::arm` opens a window (10 min by default, capped at 60)
that closes by itself. When the store is not armed, the request path only does
one atomic load. While armed, each bridged request records:

- the client request: method, path, headers and JSON body;
- the upstream request of the attempt that succeeded: URL, final headers and
  body. It is recorded in `RequestForwarder::forward` through
  `RequestScope.capture`;
- the upstream body. Streams go through `tap_stream`, which keeps each chunk
  with its offset in ms;
- the converted client body. Streams are tapped in `transformed_sse_response`,
  and JSON is recorded before `rebuilt_json_response`.

Headers and JSON bodies are redacted when they are recorded. Stream bytes are
joined and redacted at export time, so a key split across chunks is still
caught. Requests that are not bridged are dropped once the route plan is known,
and so are requests that fail to forward. Captures stay in memory only: at most
32 entries and 8 MiB per stream direction, with truncation flagged. When usage
is written, `log_usage_internal` now returns the `request_id`. That id links
the capture to its request log row.

The export writes a zip laid out as `<bridge>/<case>/`: `meta.toml`
(intent, `source_commit = v<version>`, and the `codex_oauth`,
`upstream_error` and `preserve_reasoning_content` options, plus
`invariants.terminal` inferred from the captured client stream),
`client_request.json`, `upstream_response.sse|json`,
`captured_client_response.sse|json` and `capture.json`. `capture.json` holds the
headers, the upstream request, the status and the timings of every chunk.
`meta.toml` starts with comment lines that give the replay command
(`INSTA_UPDATE=always cargo test --test proxy_golden`).

Redaction (`capture/redact.rs`):

- Sensitive headers are dropped by name: authorization, x-api-key,
  x-goog-api-key, cookie and set-cookie, and any name that contains auth,
  token, secret, api_key or cookie.
- JSON string values under sensitive keys are dropped.
- In text, these shapes are replaced: `sk-`/`sk-ant-`, `AIza`,
  `gh[pousr]_`, `github_pat_`, `Bearer <token>`, header lines, quoted
  key/value pairs (also when escaped) and URL `key=`/`token=` query parameters.

UI: the request log toolbar has a "Capture bug reports" toggle that shows the
minutes left. Captured rows get an "Export bug report" icon button. The backend
command opens the save dialog itself, so the webview never supplies a path.
The four new commands are `start_proxy_capture`, `stop_proxy_capture`,
`get_proxy_capture_status` and `export_proxy_bug_report`. They are registered
in `lib.rs`, and the generated `COMMANDS` were refreshed with
`UPDATE_BINDINGS=1`. They are listed in `tests/msw/unmockedCommands.ts`.
UI strings are in all four locales.

**Files.**
- New: `src-tauri/src/proxy/capture/{mod,redact,export,tests}.rs`,
  `src-tauri/tests/proxy_capture.rs`, `src/components/usage/BugReportCapture.tsx`.
- Changed in the backend: `src-tauri/src/proxy/{mod,server,handler_context,handlers,response_processor,forwarder}.rs`,
  `src-tauri/src/proxy/forwarder/{tests,wire_golden}.rs` (add `capture: None`),
  `src-tauri/src/proxy_harness.rs` (each harness gets its own capture store and
  exposes `capture()`), `src-tauri/src/commands/proxy.rs` and `src-tauri/src/lib.rs`.
- Changed in the frontend: `src/shared/bindings.ts`, `src/types/proxy.ts`,
  `src/lib/api/proxy.ts`, `src/lib/query/proxy.ts`,
  `src/components/usage/RequestLogTable.tsx`, `src/i18n/locales/{en,zh,zh-TW,ja}.json`,
  `tests/components/RequestLogTable.test.tsx` and `tests/msw/unmockedCommands.ts`.

**Commits.** `66cd4fe` has the feature and its tests. The commit that adds these
notes follows it.

**Verification.**
- The redaction corpus is `capture_export_has_zero_key_hits`. It seeds
  sk-ant-, sk-proj-, sk-, AIza, ghu_, ghp_, Bearer, x-api-key and cookie /
  set-cookie values into headers, the URL, both bodies, a stream key split
  across two chunks and an escaped JSON key. The test then checks every zip
  entry, and the raw zip, for each literal and each key shape: 0 hits.
  `bug_report_capture_of_a_bridged_stream_replays_green` checks the same
  end to end for the proxy token, the provider key and a key pasted into the
  prompt.
- Replay: in `bug_report_capture_of_a_bridged_stream_replays_green`, a real
  `ProxyServer` serves an Anthropic client through an openai_chat upstream.
  The test exports the zip, unpacks it and loads it with
  `proxy_testkit::load_fixture`. `run_fixture` returns zero failures (all
  invariants and the split fuzz from proxy_golden). The replayed client
  stream equals the bytes the proxy sent, and the upstream SSE is kept byte
  for byte. `bug_report_capture_of_a_non_stream_bridge_exports_json_bodies`
  covers the anthropic__responses JSON case. `capture_of_a_golden_case_replays_green`
  re-captures `anthropic__openai_chat/parity_text_stop`.
- `cargo test --manifest-path src-tauri/Cargo.toml capture` passes: lib 12
  passed, `proxy_capture` 3 passed, and 1 more matching test.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast` has 0 failures
  (lib 2339 passed, 3 ignored; every integration binary ok, proxy_golden 6/6,
  proxy_e2e 22/22).
- `cargo fmt --check` is clean. `cargo clippy -- -D warnings` and
  `cargo clippy --tests -- -D warnings` are clean.
- `pnpm typecheck` is clean. `pnpm test:unit` has 154 files and 1504 tests
  passing, including the new RequestLogTable capture tests. `pnpm format:check`
  is clean.
- `pnpm lint` reports 20 errors, all in files this change does not touch
  (`App.tsx`, `DeepLinkImportDialog.tsx`, `ProviderActions.tsx`).
- Not verified here: the native save dialog of `export_proxy_bug_report` was
  not exercised in a desktop session. It uses the same `tauri_plugin_dialog`
  call pattern as `save_file_dialog`.


**Review.** An independent review checked each acceptance item against the code
and re-ran the verification. It found one redaction leak and fixed it. Stream
bytes are redacted as raw SSE text, where a newline is still the JSON escape
`\n`. The bare-key rules (`sk-`, `AIza`, `gh?_`, `github_pat_`), the `Bearer`
rule and the header-line rule were anchored with `\b`. No word boundary exists
between the `n` of `\n` and the first letter of a key, so a key at the start of
a line in model output reached the export unredacted (for example a code block
with `\nsk-ant-...`). These rules now start with an explicit boundary group,
`KEY_START`, that also accepts a JSON escape. The group is captured and kept in
the replacement. `capture_redact_text_catches_keys_after_json_escapes` covers
`\n`, `\r\n` and `\t` before each key shape. Re-verified: `cargo test capture`
(lib 13 passed), the full `cargo test --no-fail-fast` (lib 2340 passed, 0
failures in any binary), `cargo fmt --check`, `cargo clippy` (with and without
`--tests`) at `-D warnings`, `pnpm typecheck`, `pnpm test:unit` (1504 passed)
and `pnpm format:check`.
