# CCS-148: [apps] Migrate Codex to AppAdapter, keeping the history-migration ids and markers stable

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p2-medium`, `area:apps`

> **Ticket key:** `p4-adapter-codex` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** L · **Depends on:** `p4-adapter-trait-gemini-grok`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
The critique notes codex_history_migration.rs (2630 lines) writes threads.model_provider into Codex's own state DB using CC_SWITCH_CODEX_MODEL_PROVIDER_ID, and stores markers in settings.json. Moving Codex code must not change these.

## Evidence
- codex_history_migration.rs:644, 1157; codex_config.rs:244-250 (paired rollback)

## Proposed change
Add apps/codex, which keeps the provider id constant and the marker keys. Keep the auth.json and config.toml pair rollback.

## Acceptance criteria / verification
- [ ] The Codex state DB snapshot is unchanged
- [ ] A conformance check asserts the provider id constant
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml codex`

## Dependencies
p4-adapter-trait-gemini-grok
