# CCS-092: [usage] Make AppType::input_semantics() the single source and fix the grokbuild backfill cost

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p1-high`, `area:usage`

> **Ticket key:** `p1-usage-input-semantics` · **Phase:** P1 · **Priority:** p1-high · **Effort:** S · **Depends on:** `p0-cost-golden-table`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
The cache-inclusive/exclusive input rule is copied in 5 places that disagree, so the grokbuild backfill computes a different cost from ingest.

## Evidence
- proxy/usage/calculator.rs:62, proxy/usage/logger.rs:75, sql_helpers.rs:19, usage_stats.rs:377 and :1820

## Proposed change
Add `AppType::input_semantics()` and have all 5 sites call it. The SQL helper generates its CASE from the enum.

## Acceptance criteria / verification
- [x] The cost golden table is fully green, with the #[ignore] removed from the grokbuild rows
- [x] `grep -rn 'cache-inclusive\|CACHE_INCLUSIVE' src-tauri/src` shows a single definition
- [x] `cargo test --manifest-path src-tauri/Cargo.toml cost_golden`

## Dependencies
p0-cost-golden-table

## Implementation notes

**Summary.** Added `InputSemantics { CacheInclusive, CacheExclusive }` and
`AppType::input_semantics()` (exhaustive match: Codex/Gemini/GrokBuild include
cache in `input_tokens`; everything else is fresh) in `app_config.rs`, plus
`InputSemantics::for_app_type_str()` for stored `app_type` strings (unknown
values fall back to cache-exclusive, matching the previous safe default). All
cost-semantics sites now call it:

- `proxy/usage/calculator.rs` `calculate_for_app` (ingest)
- `proxy/usage/logger.rs` `log_request` (stamps `input_token_semantics`)
- `services/sql_helpers.rs` `fresh_input_sql` — the `app_type IN (...)` list is
  generated from `AppType::all()` filtered by `input_semantics()`;
  `CACHE_INCLUSIVE_APP_TYPES` was removed
- `services/usage_stats.rs` zero-cost backfill — this was the bug: it only
  matched `codex | gemini`, so grokbuild LEGACY/TOTAL rows were billed with
  cache tokens as fresh input

`usage_stats.rs:377` (`allow_missing_cache_creation`, `codex | gemini | opencode`)
was reviewed and intentionally left as is: it describes which session-log
sources may omit `cache_creation` for dedup matching, not input semantics
(opencode is cache-exclusive), so routing it through `input_semantics()` would
change dedup behaviour. A comment there now says so.

Tests: the `#[ignore]` grokbuild backfill test was replaced by
`cost_golden_backfill_path_grokbuild_cache_rows` (regression; fails with the old
`codex | gemini` match: e.g. `grokbuild/Total/large-x2: expected 6.0, got
7.875000`), and `cost_golden_backfill_path` now runs every row. Added
`cost_golden_expected_semantics_match_app_type_input_semantics` (independent
oracle vs. the single source) and
`sql_helpers::fresh_input_app_list_matches_app_type_input_semantics`.

**Files.** `src-tauri/src/app_config.rs`, `src-tauri/src/proxy/usage/calculator.rs`,
`src-tauri/src/proxy/usage/logger.rs`, `src-tauri/src/proxy/usage/cost_golden.rs`,
`src-tauri/src/services/sql_helpers.rs`, `src-tauri/src/services/usage_stats.rs`.

**Commits.** `07178fe` (code + tests); ticket notes in the follow-up commit.

**Verification.**
- `grep -rn 'cache-inclusive\|CACHE_INCLUSIVE' src-tauri/src` → one hit, the
  `InputSemantics::CacheInclusive` definition in `app_config.rs`
- `cargo test --manifest-path src-tauri/Cargo.toml --lib -- cost_golden sql_helpers calculator`
  → 19 passed, 0 failed, 0 ignored (all cost golden rows green, no `#[ignore]`)
- Regression check: temporarily restoring the old backfill match made both
  backfill golden tests fail on the 4 grokbuild LEGACY/TOTAL rows; restored after
- `cargo fmt` clean; `cargo clippy -- -D warnings` clean
- `cargo test --lib` → 2256 passed, 0 failed (2 ignored: live S3 tests)
- `cargo test --no-fail-fast` (full) → 2422 passed, 0 failed

**Review.** Independently re-verified `07178fe`: every former copy of the rule
now calls `AppType::input_semantics()` (grep finds no remaining
`"codex" | "gemini"` input-semantics match; the `allow_missing_cache_creation`
dedup list is a separate rule and correctly left alone). Re-applying the old
`codex | gemini` backfill match makes `cost_golden_backfill_path` and
`cost_golden_backfill_path_grokbuild_cache_rows` fail, so the tests do assert
the fix. fmt/clippy clean, `cargo test --lib` 2256 passed / 0 failed, full
`cargo test --no-fail-fast` 2422 passed / 0 failed. Known limitation (out of
scope): the backfill only touches zero-cost rows, so grokbuild rows already
backfilled with the old rule keep their overcharged cost until re-priced.
Review follow-up only ticked the acceptance boxes and added this note.
