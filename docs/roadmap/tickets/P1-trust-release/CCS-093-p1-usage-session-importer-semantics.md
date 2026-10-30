# CCS-093: [usage] Persist input_token_semantics and pricing_model from session importers, and fix the dead Gemini UPSERT

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p2-medium`, `area:usage`

> **Ticket key:** `p1-usage-session-importer-semantics` · **Phase:** P1 · **Priority:** p2-medium · **Effort:** S · **Depends on:** `p1-usage-input-semantics`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Session-log importers do not persist the semantics or pricing model, so recomputation and backfill guess. The Gemini importer has an UPSERT that never matches.

## Evidence
- services/session_usage_gemini.rs:263, 300-318; session_usage.rs insert_session_log_entry

## Proposed change
Write input_token_semantics and pricing_model on every session insert, then fix or delete the Gemini UPSERT. Use the existing columns, so no schema change is needed.

## Acceptance criteria / verification
- [x] Tests: imported Claude, Codex and Gemini rows have non-null semantics and pricing_model; a Gemini re-import updates instead of duplicating
- [x] `cargo test --manifest-path src-tauri/Cargo.toml session_usage`

## Dependencies
p1-usage-input-semantics

## Implementation notes

**Summary.** Every session-log importer (Claude `insert_session_log_entry`,
Codex `insert_codex_session_entry`, Gemini `insert_gemini_session_entry`,
OpenCode `insert_opencode_message`) now writes `pricing_model` and
`input_token_semantics` using the existing columns (no schema change).
Semantics come from the new `sql_helpers::input_token_semantics_for_app_type`,
derived from `AppType::input_semantics()` (CCS-092); the proxy logger now uses
the same helper. `pricing_model` is the model name actually passed to the
pricing lookup (Codex: `normalize_codex_model(model)`; others: the session
model).

Gemini UPSERT: the importer called `should_skip_session_insert`, which returns
`true` as soon as the `request_id` exists, so the `ON CONFLICT DO UPDATE`
branch was dead and re-read files with updated tokens were dropped. It now
dedups only via `has_matching_proxy_usage_log` (proxy rows), and the UPSERT
also updates `pricing_model` / `input_token_semantics`; its `WHERE` clause
fires on those columns too, so pre-fix rows (NULL / LEGACY) get upgraded on
the next re-read. Identical re-reads still produce no change.

**Files.** `src-tauri/src/services/sql_helpers.rs`,
`src-tauri/src/proxy/usage/logger.rs`, `src-tauri/src/services/session_usage.rs`,
`src-tauri/src/services/session_usage_codex.rs`,
`src-tauri/src/services/session_usage_gemini.rs`,
`src-tauri/src/services/session_usage_opencode.rs`.

**Commits.** `ac240b1` (code + tests); ticket notes in the follow-up commit.

**Tests added.**
`test_insert_{claude,codex,gemini}_session_persists_semantics_and_pricing_model`,
`test_insert_opencode_message_persists_semantics_and_pricing_model`,
`test_gemini_reimport_updates_existing_row_instead_of_duplicating`,
`test_gemini_reimport_upgrades_legacy_row_semantics`.

**Verification.**
- `cargo test --manifest-path src-tauri/Cargo.toml --lib session_usage` → 47 passed, 0 failed
- Regression check: temporarily restoring the `should_skip_session_insert` call
  in the Gemini importer made both Gemini re-import tests fail; restored after
- `cargo fmt` clean; `cargo clippy -- -D warnings` clean
- `cargo test --lib` → 2262 passed, 0 failed (2 ignored: live S3 tests)
