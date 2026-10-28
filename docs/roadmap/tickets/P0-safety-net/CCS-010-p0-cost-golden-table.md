# CCS-010: [usage] Add a cost golden table that runs every cost path

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p1-high`, `area:usage-tests`

> **Ticket key:** `p0-cost-golden-table` · **Phase:** P0 · **Priority:** p1-high · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
The cache-inclusive/exclusive input rule is copied in 5 places that disagree. The grokbuild backfill computes a different cost from ingest. The 1-hour TTL accounting was shipped and reverted (13e7c1f/6eb217b).

## Evidence
- src-tauri/src/proxy/usage/calculator.rs:62, proxy/usage/logger.rs:75, database sql_helpers.rs:19 (fresh_input_sql), usage_stats.rs:377 and :1783-1871 (backfill)

## Proposed change
Add a table-driven test with rows of (app_type x {LEGACY, cache-inclusive, cache-exclusive} x usage x pricing row) -> expected cost, run through all four paths. Mark the failing grokbuild backfill rows #[ignore] with an issue link.

## Acceptance criteria / verification
- [x] Covers all 8 apps x 3 semantics
- [x] The only failures are the #[ignore] grokbuild rows
- [x] `cargo test --manifest-path src-tauri/Cargo.toml cost_golden`

## Dependencies
None.

## Implementation notes

**Summary.** Added `src-tauri/src/proxy/usage/cost_golden.rs` (test-only module). The golden table has 8 apps (`AppType::all()`) x 3 input-token semantics (LEGACY=0, TOTAL/cache-inclusive=1, FRESH/cache-exclusive=2) x 2 usage + pricing-row scenarios ("small" with multiplier 1, "large-x2" with multiplier 2), which gives 48 rows with literal expected totals. Each row goes through these paths:

1. **ingest**: `CostCalculator::calculate_for_app`.
2. **logger**: `UsageLogger::log_with_calculation` against a seeded `model_pricing` row. The test checks both the stamped `input_token_semantics` and the stored `total_cost_usd`.
3. **aggregation**: `fresh_input_sql("l")` over a stored row, then the cost is computed from the pricing row.
4. **backfill**: `Database::backfill_missing_usage_costs` on zero-cost rows.

Ingest and logger only ever write the semantics their app maps to, so those two paths run only the 16 rows that match the app. `ingest_semantics()` is an exhaustive `match` on `AppType`, so adding a new app will not compile until someone classifies it. Aggregation and backfill run all 48 rows.

The grokbuild LEGACY/TOTAL rows fail on the backfill path because `usage_stats.rs` `maybe_backfill_log_costs` only treats `codex|gemini` as cache-inclusive. They run in `cost_golden_backfill_path_grokbuild_known_divergence`, which is `#[ignore]`d with a link to CCS-092 (`docs/roadmap/tickets/P1-trust-release/CCS-092-p1-usage-input-semantics.md`). CCS-092 should remove the `#[ignore]` and the `is_known_backfill_divergence` filter.

**Further finding for CCS-092 (not in the table).** For malformed rows where the cache tokens exceed input, `fresh_input_sql` keeps the raw `input_tokens` (`input_tokens >= cache...` guard). The calculator and backfill instead clamp to 0 with `saturating_sub`. The paths therefore disagree on such rows.

**Files:** `src-tauri/src/proxy/usage/cost_golden.rs` (new), `src-tauri/src/proxy/usage/mod.rs` (`#[cfg(test)] mod cost_golden;`).

**Commits:** c063fbd (tests). The ticket-notes commit follows it.

**Verification:**
- `cargo test --manifest-path src-tauri/Cargo.toml --lib cost_golden`: 4 passed, 1 ignored.
- `cargo test ... --lib cost_golden -- --ignored`: the one ignored test fails, and only on the 4 grokbuild LEGACY/TOTAL backfill rows (expected 0.010335/0.010035/6.625/6.0, got 0.010935/0.010935/7.875/7.875).
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 2078 passed, 0 failed, 3 ignored.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`: clean. `cargo fmt --check`: clean.
