# CCS-162: [config] Keep user key order when rewriting user-owned JSON configs

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P4`, `p3-low`, `area:config`

> **Ticket key:** `p4-json-key-order` · **Phase:** P4 · **Priority:** p3-low · **Effort:** S · **Depends on:** `p4-adapter-additive`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding backend-json-key-resort, partially real. write_json_file sorts keys on purpose. A user-ordered ~/.claude/settings.json gets a one-time reorder diff. preserve_order is already enabled, so the proposed fix of enabling it does nothing.

## Evidence
- src-tauri/src/config.rs:256-286 (sort_json_keys); Cargo.toml:26 preserve_order

## Proposed change
- Add write_json_file_sorted for files cc-switch owns.
- Adapters for user-owned live files call an unsorted writer and rely on read-modify-write merges.
- Alternatively, record a won't-fix decision.

## Acceptance criteria / verification
- [ ] Test: `{"z":1,"a":{"y":2,"b":3}}` round-trips unchanged through the user-file writer
- [ ] Writer snapshots updated intentionally
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml config::`

## Dependencies
p4-adapter-additive
