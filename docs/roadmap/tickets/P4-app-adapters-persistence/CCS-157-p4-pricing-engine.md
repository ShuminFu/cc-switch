# CCS-157: [pricing] Add the pricing catalog, resolver and a single cost engine, and delete the hand-written backfill

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p2-medium`, `area:pricing`

> **Ticket key:** `p4-pricing-engine` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** L · **Depends on:** `p4-migration-framework`, `p1-usage-input-semantics`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Evidence
- usage_stats.rs:2004-2275 resolver; schema.rs:1516-2745 (~880-line seed and repair list); usage_stats.rs:1783-1871 backfill

## Proposed change
- pricing/catalog.json, resolver.rs and engine.rs.
- Backfill and ingest both call engine::cost.
- Remove the seed and repair code from schema.rs.

## Acceptance criteria / verification
- [x] One cost function (grep check)
- [x] The cost golden table has at least 50 rows and passes; backfill output equals ingest output for the same row
- [x] `cargo test --manifest-path src-tauri/Cargo.toml pricing cost_golden`

## Dependencies
p4-migration-framework, p1-usage-input-semantics

## Implementation notes

**Summary.** New `src-tauri/src/pricing/` module:
- `catalog.json` + `catalog.rs`: the 174 built-in pricing rows formerly hard-coded in `schema.rs::builtin_model_pricing`, converted mechanically (comments kept as `note`). The 27-entry `repair_current_model_pricing` list became per-model `supersedes[]` arrays of old prices; `catalog::sync` = `seed` (INSERT OR IGNORE, skipping user-deleted built-ins) + `apply_supersedes` (update only rows still equal to an old catalog price). Price changes now ship as data: edit the price and append the old one to `supersedes`.
- `resolver.rs`: `find_model_pricing[_row]`, `model_pricing_candidates`, the prefix/date/namespace normalizers and their tests, moved out of `services/usage_stats.rs`.
- `engine.rs`: `engine::cost(&TokenCounts, InputBasis, &ModelPricing, multiplier)` is the only cost function. `InputBasis::for_app_type` (ingest) and `InputBasis::for_stored_row` (backfill: LEGACY / TOTAL / FRESH) pick which cache buckets to subtract. `ModelPricing` / `CostBreakdown` live here; `proxy/usage/calculator.rs` re-exports them and `CostCalculator` is a thin wrapper.
- `schema.rs` lost the ~1250-line seed and repair code; `ensure_model_pricing_seeded_on_conn` calls `pricing::catalog::sync`, and the frozen legacy migrations call `pricing::catalog::seed`.
- The backfill (`usage_stats.rs::maybe_backfill_log_costs`) calls `engine::cost` and writes the Decimal strings exactly as `UsageLogger` does. Before this change it rounded to 6 dp, so sub-micro-dollar rows differed from ingest. Backfill tests now compare stored costs as Decimal values.

**Golden table.** Added a `tiny-x1.25` scenario (sub-micro-dollar costs, 1.25 multiplier), giving 72 rows (8 apps × 3 semantics × 3 scenarios), with a `>= 50` guard. The new `cost_golden_backfill_output_equals_ingest_output` test checks that the five cost columns written by logger ingest and by zero-cost backfill are byte-identical for the same row. The aggregation path now prices through `engine::cost` too. Regression check: putting the old `{:.6}` backfill formatting back makes `cost_golden_backfill_path`, `..._grokbuild_cache_rows` and `cost_golden_backfill_output_equals_ingest_output` fail. The grokbuild row noted in the plan was already fixed by CCS-092, so nothing needed `#[ignore]`.

**Grep check.** `pricing::engine::tests::pricing_engine_is_the_only_cost_function` scans `src/` and `tests/` and fails if any file other than `pricing/engine.rs` reads a `.{input,output,cache_read,cache_creation}_cost_per_million` field. Manual run: `grep -rnE "cost_per_million\s*/|/ million" src-tauri/src` → only `src/pricing/engine.rs`.

**Not in scope / follow-ups.** The provenance columns (source, catalog_version, updated_at, deleted), the `UsageRepo` / `PricingRepo` split and integer micro-USD rollups from the plan's P4 row are not in this ticket's proposed change and were left alone. User deletions of built-ins are still tracked in settings (`deleted_builtin_model_pricing`). Known pre-existing gap, unchanged: `fresh_input_sql` returns raw input when input < cache tokens, while the engine saturates to 0. The golden scenarios keep input ≥ cache.

**Files.** `src-tauri/src/pricing/{mod.rs,catalog.rs,catalog.json,engine.rs,resolver.rs}`, `src-tauri/src/lib.rs`, `src-tauri/src/database/schema.rs`, `src-tauri/src/database/migrations/legacy.rs`, `src-tauri/src/proxy/usage/{calculator.rs,cost_golden.rs,logger.rs}` (logger: import line only), `src-tauri/src/services/{usage_stats.rs,session_usage.rs,session_usage_codex.rs,session_usage_gemini.rs,session_usage_opencode.rs}`.

**Commits.** `1e1de66` (code), plus this ticket-notes commit.

**Verification.**
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check` → clean
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` → clean
- `cargo test --manifest-path src-tauri/Cargo.toml -- pricing cost_golden` → 36 + 1 passed, 0 failed. The ticket's form without `--` is rejected by cargo ("unexpected argument 'cost_golden'"), because more than one filter has to go after `--`.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib` → 2383 passed, 0 failed, 2 ignored
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast` → 2582 passed, 0 failed
