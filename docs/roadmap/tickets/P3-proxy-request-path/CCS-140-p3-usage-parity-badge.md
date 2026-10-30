# CCS-140: [usage] Show a parity badge when normalized usage disagrees with the raw upstream usage

**Status:** Done · **Kind:** feature · **Labels:** `feature`, `phase:P3`, `p3-low`, `area:usage`

> **Ticket key:** `p3-usage-parity-badge` · **Phase:** P3 · **Priority:** p3-low · **Effort:** S · **Depends on:** `p3-usage-ir`, `p3-stream-traits-taps`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Double-counted cache_read and zero-usage streams reach users' bills unnoticed.

## Proposed change
Compare the UsageIR billable totals with the raw upstream usage block. Store a mismatch flag on the log row (in an existing JSON metadata column, with no DDL) and show a badge with details.

## Acceptance criteria / verification
- [x] A fixture with deliberately inconsistent usage sets the flag
- [x] RTL: the badge renders
- [x] `cargo test --manifest-path src-tauri/Cargo.toml usage` and `pnpm test:unit`

## Dependencies
p3-usage-ir, p3-stream-traits-taps

## Implementation notes

**Summary.** `src-tauri/src/proxy/usage/parity.rs` re-sums the raw upstream usage block from its own fields, without going through `UsageIR`. It reads `total_tokens` or `totalTokenCount` when present, or else the sum of the fields, together with the reported input and the cached read/write tokens. It then compares that with the normalized billable total of the `TokenUsage` being logged. Three issues are flagged:
- `zero_usage`: a successful request with 0 billable tokens.
- `cache_exceeds_input`: in OpenAI/Gemini semantics, cached tokens exceed the reported input, so cache_read was double-counted.
- `total_mismatch`: the upstream total differs from the billed total.

`UsageLogger::with_usage_parity(raw)` turns the check on for the proxy path only, and only for 2xx rows. Session imports and error rows are never flagged.

**Storage (no DDL).** `proxy_request_logs` has no JSON metadata column. Success rows always have `error_message = NULL`, so the flag is written there as `{"usageParity":{...}}` JSON metadata. `row_to_request_log_detail` uses `decode_log_metadata` to split it back out into `RequestLogDetail.usage_parity`. `error_message` then still reads as `null`, and rows with status 400 or above are never decoded. A dedicated column can replace this once numbered migrations land. One caveat: an older app version that reads a synced DB would show the JSON text as the error message of that row.

**Raw block plumbing.**
- Passthrough non-stream: the response's `usage` / `usageMetadata` block.
- Passthrough stream: `UsageTap::raw_usage()`, which keeps the single block that `usage()` read. That is the last Chat usage chunk, the locked Codex `response.completed` usage, or the last Gemini `usageMetadata`. Anthropic streams split usage across two events, so only the zero-usage check applies to them.
- Transformed non-stream (Claude ← OpenAI/Responses/Gemini, Codex ← Chat, Codex ← Anthropic): the upstream block, cloned before conversion.
- Transformed streams: the client-format block.

**UI.** `UsageParityBadge` shows a "Usage mismatch" badge next to the status code in the request log. The title and aria-label carry the issue and the billed vs upstream totals (input, cache). It is translated in en/zh/zh-TW/ja.

**Files.** `src-tauri/src/proxy/usage/{parity.rs,mod.rs,logger.rs}`, `src-tauri/src/proxy/stream/taps.rs`, `src-tauri/src/proxy/response_processor.rs`, `src-tauri/src/proxy/handlers.rs`, `src-tauri/src/services/usage_stats.rs`, `src/shared/bindings.ts` (regenerated), `src/components/usage/{UsageParityBadge.tsx,RequestLogTable.tsx}`, `src/i18n/locales/*.json`, `tests/components/RequestLogTable.test.tsx`, `tests/i18n/usedKeys.test.ts`.

**Commits.** `95dea41` (feature), plus this ticket update.

**Verification.**
- Fixture tests: `logger::tests::inconsistent_upstream_usage_sets_parity_flag_on_log_row` uses `cached_tokens` 150 greater than `prompt_tokens` 100, which sets `cache_exceeds_input` with 170 normalized vs 120 raw. `response_processor::tests::test_log_usage_flags_inconsistent_upstream_usage` sets `total_mismatch` with 60 vs 90. There are also parity unit tests and `taps::tests::raw_usage_tracks_the_block_usage_is_read_from`.
- RTL: the `usage parity badge (CCS-140)` tests in `tests/components/RequestLogTable.test.tsx` check that the badge renders, with its details, only on mismatched rows.
- `cargo test --manifest-path src-tauri/Cargo.toml usage`: 204 passed, 0 failed (lib).
- Full `cargo test --no-fail-fast`: 2528 passed, 1 failed. The failure was `tests/migrations.rs::every_released_schema_migrates_to_current`. That code is untouched here, and the test passed on 3 reruns, so it is treated as flaky under full-suite load.
- `cargo clippy -- -D warnings` and `--all-targets`: clean. `cargo fmt`: clean.
- `pnpm typecheck`: clean. `pnpm test:unit`: 155 files, 1509 tests passed. `pnpm format:check`: clean.
- `UPDATE_BINDINGS=1 cargo test --lib bindings`: regenerated, up to date.

**Pre-existing issue found (not fixed, out of scope).** `UsageStats::get_request_detail` (the `get_request_detail` command) fails with `ambiguous column name: created_at`. Its SELECT uses an unqualified `created_at` while joining `providers`, which also has a `created_at` column. `RequestDetailPanel` is currently unused, so the tests here read rows through `get_request_logs`.

**Review.** Every acceptance item checks out against the code. One false positive was fixed. Every successful proxied request that returns no usage gets logged as a zero-usage row, and the Gemini route also forwards non-generation calls (`:countTokens`, GET `models` / `models/<id>`). So every such row was flagged `zero_usage` and showed the "Usage mismatch" badge. `UsageLogger` now skips the check for a non-streaming response that has no usage block at all. A stream with no usage, and a non-streaming usage block that is all zeros, are still flagged. The logger fixture test covers all three cases. Re-verified: `cargo test --lib parity` 10 passed. Full `cargo test --no-fail-fast` 2529 passed, 0 failed, and `tests/migrations.rs` passed this time too. `cargo clippy --all-targets -- -D warnings` and `cargo fmt` are clean. `pnpm typecheck` is clean, and the RequestLogTable and i18n vitest suites passed.
