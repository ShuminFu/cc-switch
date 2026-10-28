# CCS-150: [apps] Add an adapter conformance suite that loops over APPS, plus the dummy 9th-app test

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P4`, `p1-high`, `area:apps-tests`

> **Ticket key:** `p4-adapter-conformance` · **Phase:** P4 · **Priority:** p1-high · **Effort:** M · **Depends on:** `p4-adapter-additive`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
Add tests/app_adapter_conformance.rs, which for each app checks:
- credentials round-trip
- MCP upsert and remove
- prompt and skills paths
- takeover apply then cleanup restores the original bytes
- caps match the implemented traits
- the codex model_provider id is stable

Also add a test-only 9th app.

## Acceptance criteria / verification
- [ ] Passes for all 8 apps
- [ ] The dummy app touches 6 or fewer backend files and needs no migration (documented in the PR)
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test app_adapter_conformance`

## Dependencies
p4-adapter-additive
