# CCS-093: [usage] Persist input_token_semantics and pricing_model from session importers, and fix the dead Gemini UPSERT

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p2-medium`, `area:usage`

> **Ticket key:** `p1-usage-session-importer-semantics` · **Phase:** P1 · **Priority:** p2-medium · **Effort:** S · **Depends on:** `p1-usage-input-semantics`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Session-log importers do not persist the semantics or pricing model, so recomputation and backfill guess. The Gemini importer has an UPSERT that never matches.

## Evidence
- services/session_usage_gemini.rs:263, 300-318; session_usage.rs insert_session_log_entry

## Proposed change
Write input_token_semantics and pricing_model on every session insert, then fix or delete the Gemini UPSERT. Use the existing columns, so no schema change is needed.

## Acceptance criteria / verification
- [ ] Tests: imported Claude, Codex and Gemini rows have non-null semantics and pricing_model; a Gemini re-import updates instead of duplicating
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml session_usage`

## Dependencies
p1-usage-input-semantics
