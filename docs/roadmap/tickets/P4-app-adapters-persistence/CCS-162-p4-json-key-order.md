# CCS-162: [config] Keep user key order when rewriting user-owned JSON configs

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P4`, `p3-low`, `area:config`

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
- [x] Test: `{"z":1,"a":{"y":2,"b":3}}` round-trips unchanged through the user-file writer
- [x] Writer snapshots updated intentionally
- [x] `cargo test --manifest-path src-tauri/Cargo.toml config::`

## Dependencies
p4-adapter-additive

## Implementation notes

**Summary.** `write_json_file` (src-tauri/src/config.rs) now writes keys in their existing order (serde_json `preserve_order` was already enabled), so the read-modify-write merges used by every user-owned live-config writer keep the user's key order and no longer produce a one-time reorder diff. The previous alphabetical behaviour lives on as `write_json_file_sorted`, used only for files cc-switch generates wholesale:
- `~/.cc-switch/config.json` (`MultiAppConfig::save`, src-tauri/src/app_config.rs)
- the generated Codex model catalog (src-tauri/src/apps/codex/config.rs)
- the Claude Desktop gateway profile cc-switch generates (src-tauri/src/apps/claude_desktop/config.rs); Claude Desktop's own `claude_desktop_config.json` and meta file are user/app-owned and use the order-preserving writer.

Both writers share a private `write_json_value` helper (directory creation, capture-preview handling and `atomic_write` unchanged).

**Tests.**
- `config::tests::write_json_file_round_trips_user_key_order_unchanged`: `{"z":1,"a":{"y":2,"b":3}}` round-trips byte-for-byte through the user-file writer (verified to fail with the old sorting writer).
- `config::tests::write_json_file_sorted_sorts_keys_recursively`: the owned-file writer still sorts recursively.
- `capture_writes_preview_records_writes_without_touching_disk` expectation updated to insertion order.
- Writer snapshots in `src-tauri/tests/fixtures/live/{claude_settings,claude_desktop,gemini,opencode}/*.snap` regenerated intentionally (`INSTA_UPDATE=always cargo test --test config_writers`); the diffs only move keys back into the fixture's original order.

**Commits.** 735adcd (code, tests, snapshots); this ticket update in a follow-up commit.

**Verification.**
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: clean
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: clean
- `cargo test --manifest-path src-tauri/Cargo.toml --lib config::`: 233 passed, 0 failed
- `cargo test --manifest-path src-tauri/Cargo.toml --test config_writers`: 18 passed, 0 failed
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: 2617 passed, 0 failed
