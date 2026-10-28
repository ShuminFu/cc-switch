# CCS-205: [provider-form] Preview the diff of the target live config before saving

**Status:** Open · **Kind:** feature · **Labels:** `feature`, `phase:F`, `p3-low`, `area:frontend`

> **Ticket key:** `f-live-diff-preview` · **Phase:** F · **Priority:** p3-low · **Effort:** M · **Depends on:** `f-providerform-shell`, `p4-adapter-additive`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Users save provider edits without seeing what will change in ~/.claude/settings.json, config.toml and the other live files.

## Proposed change
Add a backend command preview_live_write(app, provider) that returns the adapter output without writing it. The form shows a side-by-side diff before Save.

## Acceptance criteria / verification
- [ ] Rust test: preview equals the bytes later written
- [ ] RTL: the diff renders
- [ ] `pnpm test:unit` and `cargo test --manifest-path src-tauri/Cargo.toml preview`

## Dependencies
f-providerform-shell, p4-adapter-additive
