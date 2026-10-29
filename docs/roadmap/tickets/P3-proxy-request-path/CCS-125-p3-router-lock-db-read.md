# CCS-125: [proxy] Read the breaker config before taking the breaker-map write lock

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P3`, `p3-low`, `area:proxy`

> **Ticket key:** `p3-router-lock-db-read` · **Phase:** P3 · **Priority:** p3-low · **Effort:** S · **Depends on:** `p3-runtime-config-snapshot`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding proxy-router-lock-await, partially real. The first creation of a breaker for each key holds the global map write lock across a SQLite read, which stalls every map lookup. It happens once per key and is usually sub-millisecond.

## Evidence
- src-tauri/src/proxy/provider_router.rs:235-270; dao/proxy.rs:215-271

## Proposed change
After the read-lock miss, take the config from the runtime snapshot (or the DB), then take the write lock, check again, and insert.

## Acceptance criteria / verification
- [x] No `.await` on the db between write().await and insert
- [x] Multi-thread test: concurrent creation for the same key gives Arc::ptr_eq and one entry
- [x] The provider_router.rs:336 test still passes; `cargo test --manifest-path src-tauri/Cargo.toml provider_router`

## Dependencies
p3-runtime-config-snapshot

## Implementation notes

**Summary:** `ProviderRouter::get_or_create_circuit_breaker` now reads the per-app breaker config (via the `RuntimeConfigStore` snapshot, falling back to the DB on a miss) after the read-lock miss and *before* `circuit_breakers.write().await`. Under the write lock there is only the double-check and the insert, with no `.await`. If two tasks race, the first inserted breaker wins and the other's freshly built breaker is discarded.

**Files:**
- `src-tauri/src/proxy/provider_router.rs`: reordered config read and write lock; added tests `test_concurrent_breaker_creation_yields_single_entry` (multi-thread runtime, 16 tasks behind a barrier: all results `Arc::ptr_eq`, map has exactly one entry) and `test_breaker_creation_does_not_hold_map_lock_during_db_read` (holds the DB connection mutex so the config read blocks, then asserts `circuit_breakers.try_read()` succeeds).

**Commits:** `c949061` (code + tests), plus the follow-up commit that records these notes.

**Verification:**
- The regression test fails with the old ordering: the lock was held during the DB read, so the assertion panicked. It passes with the fix.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib provider_router`: 11 passed, 0 failed (includes `test_provider_router_creation`)
- `cargo fmt --manifest-path src-tauri/Cargo.toml`: no changes
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: clean
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 2239 passed, 0 failed, 3 ignored
