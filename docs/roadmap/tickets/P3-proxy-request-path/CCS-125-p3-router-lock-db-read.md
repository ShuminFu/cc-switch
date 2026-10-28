# CCS-125: [proxy] Read the breaker config before taking the breaker-map write lock

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P3`, `p3-low`, `area:proxy`

> **Ticket key:** `p3-router-lock-db-read` · **Phase:** P3 · **Priority:** p3-low · **Effort:** S · **Depends on:** `p3-runtime-config-snapshot`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding proxy-router-lock-await, partially real. The first creation of a breaker for each key holds the global map write lock across a SQLite read, which stalls every map lookup. It happens once per key and is usually sub-millisecond.

## Evidence
- src-tauri/src/proxy/provider_router.rs:235-270; dao/proxy.rs:215-271

## Proposed change
After the read-lock miss, take the config from the runtime snapshot (or the DB), then take the write lock, check again, and insert.

## Acceptance criteria / verification
- [ ] No `.await` on the db between write().await and insert
- [ ] Multi-thread test: concurrent creation for the same key gives Arc::ptr_eq and one entry
- [ ] The provider_router.rs:336 test still passes; `cargo test --manifest-path src-tauri/Cargo.toml provider_router`

## Dependencies
p3-runtime-config-snapshot
