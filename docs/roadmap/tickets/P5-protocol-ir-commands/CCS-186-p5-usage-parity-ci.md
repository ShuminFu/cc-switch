# CCS-186: [usage] Run a usage parity checker over the fixture corpus in CI

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P5`, `p3-low`, `area:usage-tests`

> **Ticket key:** `p5-usage-parity-ci` · **Phase:** P5 · **Priority:** p3-low · **Effort:** S · **Depends on:** `p3-usage-parity-badge`, `p5-codec-responses`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
A test that, for every golden fixture, compares the UsageIR-normalized totals with the raw upstream usage and the per-semantics cost table.

## Acceptance criteria / verification
- [x] 0 mismatches; CI fails on any mismatch
- [x] `cargo test --manifest-path src-tauri/Cargo.toml usage_parity`

## Dependencies
p3-usage-parity-badge, p5-codec-responses

## Implementation notes

**Summary.** `proxy_testkit::usage_parity_with(fixture, engine)` checks every upstream body of a golden fixture: the unsplit stream body and the non-stream body. For each one it takes the raw upstream usage block. That is the last Chat usage chunk, the last Gemini `usageMetadata`, or the last Responses object with usage. For Anthropic it is `message_start` and `message_delta` merged, with the later value winning. `check_usage_parity_case` then checks:
- **Totals:** the UsageIR-normalized usage (`UsageIR::from_<upstream>(raw)`) and the usage the proxy records for the client output must both re-sum to the raw block's own totals. This uses `usage::parity::check_usage_parity`. A zero usage passes only when the raw block is zero too.
- **Buckets:** the recorded prompt total, cache read, cache write and output must equal the IR's.
- **Cost table:** the IR is priced bucket by bucket from distinct per-bucket prices, independently of the engine. That price must equal the `pricing::engine::cost` result for each of these rows:
  - the IR under each stored input semantics: fresh, total, and legacy (cache-read-only);
  - the IR's usage via `CostCalculator::calculate_for_app` under the upstream's app type;
  - the recorded usage under the client's app type.
- **No usage block:** if the upstream sent no usage block, the recorded usage must not be billable.

**Two documented exceptions, both explicit:**
- **Aborted client streams** (no completion event). Anthropic usage takes the output from `message_delta`, which an aborted stream never sends. So the recorded usage only has to stay within the IR in each bucket, and the IR checks still apply. This covers `anthropic__gemini/stream_upstream_error`, where output is 0 in the recorded usage vs 1 in the IR.
- **Upstreams whose own usage block is inconsistent.** These declare `[usage_parity] expected_issue = "<flag>"` plus a `reason` in `meta.toml`, and the checker then requires exactly that flag. Only `anthropic__openai_chat/stream_usage_cache_exceeds_prompt` declares one: `cache_exceeds_input`, because cache 130 is greater than `prompt_tokens` 100. That matches the runtime badge from CCS-140.

No snapshot changed.

**CI.** `tests/proxy_golden.rs::usage_parity_golden_corpus` runs the checker over all five bridges under both the Legacy and IR engines. It fails on any mismatch, and also if fewer than 100 bodies with usage were checked. The current count is 108 per engine. CI's existing `Run tests` step (`cargo test --manifest-path src-tauri/Cargo.toml` in `ci.yml`) runs it, so the workflow needs no change.

**Files.** `src-tauri/src/proxy/providers/testkit.rs` (checker, `UsageParityConfig` meta, unit tests), `src-tauri/tests/proxy_golden.rs`, `src-tauri/tests/fixtures/proxy/anthropic__openai_chat/stream_usage_cache_exceeds_prompt/meta.toml`.

**Commits.** `f34e2f1` (checker + test), plus this ticket update.

**Verification.**
- `cargo test --manifest-path src-tauri/Cargo.toml usage_parity`: 6 lib tests passed (`testkit::tests::usage_parity_*`, which cover consistent usage, cache double count, bucket/cost drift, missing or invented usage, aborted-stream bounds and the Anthropic stream merge) and `usage_parity_golden_corpus` passed. 0 mismatches.
- Fails before, passes after: on the first run (before the two exceptions were declared) the corpus test failed with 5 mismatches across the two fixtures above. Removing the `expected_issue` from that `meta.toml` makes it fail again.
- `cargo test --no-fail-fast` (full): 2875 passed, 0 failed.
- `cargo clippy -- -D warnings` and `cargo clippy --all-targets -- -D warnings`: clean. `cargo fmt --check`: clean.
