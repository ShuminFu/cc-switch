# CCS-209: [usage] Filter and export unpriced requests, with a shortcut to add model pricing

**Status:** Done · **Kind:** feature · **Labels:** `feature`, `phase:F`, `p3-low`, `area:usage-ui`

> **Ticket key:** `f-unpriced-requests-export` · **Phase:** F · **Priority:** p3-low · **Effort:** S · **Depends on:** `p4-usage-repo`, `p0-i18n-missing-keys`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Frontend idea 10. Users cannot find which models are missing prices.

## Proposed change
- Add an "unpriced" filter on the request log and a CSV export grouped by model.
- Add an "Add pricing" action that pre-fills the model id.

## Acceptance criteria / verification
- [x] RTL: the filter shows only unpriced rows; the export contains the model ids
- [x] `pnpm test:unit`

## Dependencies
p4-usage-repo, p0-i18n-missing-keys

## Implementation notes

**Summary**
- Backend: `LogFilters` gains `unpriced: Option<bool>`; `UsageQuery::unpriced()` adds a SQL filter matching the frontend `isUnpricedUsage` rule (2xx, has tokens, `total_cost_usd = 0`, multiplier not an explicit 0). When the filter is on, `get_request_logs` first runs the existing zero-cost backfill over all candidates, so rows priced in the meantime drop out and the count/pagination only cover rows that really lack pricing.
- New command `get_unpriced_model_stats(filters)` (specta-typed) groups the remaining unpriced rows by the model id a price lookup would use (stored `pricing_model` unless it is a placeholder, else `model`): request count, token sums, first/last seen. Sorted by request count desc.
- CSV writing: `save_csv_file_dialog` registers the chosen path under the new `PickPurpose::CsvExport`; `write_csv_export(file_path, content)` writes only to such paths (same picked-path guard used by SQL export).
- Frontend (`RequestLogTable`): "Unpriced only" toggle (`aria-pressed`, resets to page 1, part of the query key); "Export unpriced CSV" calls the stats command with the current filters + range, builds the CSV (`src/components/usage/unpricedExport.ts`: RFC 4180 quoting, formula-injection guard, ISO timestamps) and saves it through the dialog; nothing to export shows an info toast. Unpriced rows show a "+" (Add pricing) button that opens `PricingEditModal` in add mode with the model id pre-filled (`getPricingModelId`); saving invalidates usage queries, so the row is re-priced via backfill.
- i18n: `usage.unpricedOnly`, `usage.exportUnpriced`, `usage.exportUnpricedEmpty`, `usage.exportUnpricedSuccess` (`count`), `usage.exportUnpricedFailed` (`detail`) in en/zh/zh-TW/ja.

**Files**
- `src-tauri/src/services/usage_stats.rs`, `src-tauri/src/database/repo/usage.rs`, `src-tauri/src/database/repo/tests.rs`, `src-tauri/src/commands/usage.rs`, `src-tauri/src/commands/import_export.rs`, `src-tauri/src/picked_paths.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/bindings.rs`, `src/shared/bindings.ts` (regenerated)
- `src/components/usage/RequestLogTable.tsx`, `src/components/usage/unpricedExport.ts` (new), `src/types/usage.ts`, `src/lib/api/usage.ts`, `src/lib/query/usage.ts`, `src/i18n/locales/*.json`
- `tests/components/RequestLogTable.test.tsx`, `tests/components/unpricedExport.test.ts` (new), `tests/lib/usageBindings.test.ts`, `tests/msw/unmockedCommands.ts`

**Commits**
- `c5f7e6a` feat(usage): filter and export unpriced requests with an add-pricing shortcut (CCS-209)
- docs commit recording these notes

**Verification**
- RTL (`tests/components/RequestLogTable.test.tsx`): toggling the filter passes `unpriced: true` and only the unpriced row remains; export calls `getUnpricedModelStats` with `unpriced: true` + current app/model/range, and the written CSV contains the model ids (including a quoted `"other,model"`); empty export opens no dialog; Add pricing opens the editor pre-filled with the pricing model id.
- Rust: `database::repo::tests::unpriced_filter_and_model_stats_only_cover_unpriced_rows` (excludes priced / failed / no-token / multiplier-0 rows and rows backfillable from new pricing; grouping, totals, combined filters); `commands::import_export::tests::csv_export_only_writes_csv_dialog_picked_paths`.
- `cargo fmt` clean; `cargo clippy -- -D warnings` clean; `cargo test --lib`: 2400 passed, 0 failed (2 ignored); `UPDATE_BINDINGS=1 cargo test --lib bindings` then `bindings_are_up_to_date` passes.
- `pnpm typecheck` clean; `pnpm format:check` clean; `pnpm test:unit`: 157 files / 1531 tests, all passing after updating the usage-bindings command count (13 -> 14).

