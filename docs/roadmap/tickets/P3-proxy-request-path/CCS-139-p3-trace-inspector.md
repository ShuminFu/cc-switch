# CCS-139: [proxy] Add a per-request trace inspector

**Status:** Done · **Kind:** feature · **Labels:** `feature`, `phase:P3`, `p2-medium`, `area:proxy-ui`

> **Ticket key:** `p3-trace-inspector` · **Phase:** P3 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p3-route-plan`, `p3-attempt-recorder`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Users cannot see why a request went to provider B. Merges plan feature with proxy review idea 5 (request inspector).

## Proposed change
- Keep a ring buffer of the last N requests: the RoutePlan, each attempt with its status, the recovery strategies applied, TTFB, the usage source and the redacted headers.
- Add a detail panel in the request log.

## Acceptance criteria / verification
- [x] E2E: a failover request shows 2 attempts with reasons
- [x] The buffer is bounded (test)
- [x] `pnpm test:unit` and `cargo test --manifest-path src-tauri/Cargo.toml trace`

## Dependencies
p3-route-plan, p3-attempt-recorder

## Implementation notes

**Summary.** New `proxy::trace` module: an always-on, bounded ring buffer (`TraceStore`, app-wide like the capture store, `MAX_TRACES = 100`) of per-request `RequestTrace`s. The handler opens a trace for every proxied request and records the redacted client headers. The forwarder records each attempt:
- provider, plus `skipped` when the circuit breaker refuses it;
- the RoutePlan summary from `forward()`: client/upstream wire format, api format, decode, URL without query and redacted, auth, quirks, side stores;
- the redacted outbound headers;
- the recovery strategies applied (`RecoveryState::applied`);
- the outcome (`succeeded` / `failed`) with the redacted, truncated reason (`get_error_message`) and the upstream status.

The successful attempt sets `ttfbMs` (upstream response headers). The usage path sets `usageSource` (`stream_events` / `response_body` / `missing`) and `firstTokenMs`, and links the trace to the request-log `request_id`. Error rows are linked from `log_forward_error`. Limits: `MAX_TRACE_ATTEMPTS = 16` attempts per trace (the rest are counted in `droppedAttempts`), header values capped at 256 chars, reasons at 1024 chars, and no request or response bodies are stored. New IPC command `get_proxy_request_trace(requestId)`. In the request log, every proxy row gets an "Inspect route" button that opens `RequestTracePanel`. The panel shows the status, TTFB, first token, latency, usage source, client headers, and each attempt with its status, reason, HTTP status, recovery, route and upstream headers. Strings are added in en/zh/zh-TW/ja.

**Files.**
- Backend:
  - new: `src-tauri/src/proxy/trace/{mod.rs,tests.rs}`, `src-tauri/tests/proxy_trace.rs`
  - changed: `src-tauri/src/proxy/{mod.rs,forwarder.rs,forwarder/tests.rs,forwarder/wire_golden.rs,handler_context.rs,handlers.rs,response_processor.rs,server.rs,pipeline/recovery.rs}`, `src-tauri/src/proxy_harness.rs` (gets a private `traces()` store), `src-tauri/src/commands/proxy.rs`, `src-tauri/src/lib.rs`
- Frontend:
  - new: `src/components/usage/RequestTracePanel.tsx`
  - changed: `src/components/usage/RequestLogTable.tsx`, `src/lib/api/proxy.ts`, `src/lib/query/proxy.ts`, `src/types/proxy.ts`, `src/i18n/locales/*.json`, `src/shared/bindings.ts` (regenerated)
- Tests:
  - new: `tests/components/RequestTracePanel.test.tsx`
  - changed: `tests/components/RequestLogTable.test.tsx`, `tests/i18n/usedKeys.test.ts` (dynamic key allowlist), `tests/msw/unmockedCommands.ts`

**Commits.** `aa42583` feat(proxy): add a per-request trace inspector (CCS-139), plus this docs commit.

**Verification.**
- `cargo test --manifest-path src-tauri/Cargo.toml trace`: 6 `proxy::trace::tests` and 2 `tests/proxy_trace.rs` tests pass. The other trace-matching test in that run (`canonical_json_sorts_object_keys_for_cache_trace_hashes`) passes too.
  - E2E `trace_of_a_failover_request_shows_two_attempts_with_reasons`: a returns 503 and b returns 200. The test checks 2 attempts (a `failed` with upstream status 503 and reason "…503…overloaded", b `succeeded`), the plan URLs, the usage source, the TTFB, and that no token or key leaks.
  - `trace_of_a_request_where_every_provider_fails_is_linked_to_the_error_row`.
  - Bounded buffer: `trace_buffer_is_bounded_and_drops_the_oldest`, `trace_buffer_respects_a_custom_capacity`, `trace_attempts_per_request_are_bounded`.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast` (whole workspace): 2517 passed, 0 failed.
- `cargo fmt --check` passes. `cargo clippy -- -D warnings` and `cargo clippy --tests --features testkit -- -D warnings` are both clean.
- `UPDATE_BINDINGS=1 cargo test --lib bindings` regenerated `src/shared/bindings.ts`; the bindings check passes. `node scripts/check-ipc-orphans.mjs`: 0 orphans.
- `pnpm typecheck` passes. `pnpm test:unit`: 155 files, 1507 tests passed. `pnpm format:check` passes.
- `pnpm lint`: no errors in the files touched here. The errors that remain are in `src/App.tsx`, `DeepLinkImportDialog.tsx` and `ProviderActions.tsx`, which this change does not touch.
