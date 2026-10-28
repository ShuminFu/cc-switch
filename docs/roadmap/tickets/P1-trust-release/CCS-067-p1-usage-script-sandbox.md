# CCS-067: [usage-script] Run usage scripts with interrupt, memory and stack limits off the tokio workers

**Status:** Open · **Kind:** security · **Labels:** `security`, `phase:P1`, `p1-high`, `area:usage-script`

> **Ticket key:** `p1-usage-script-sandbox` · **Phase:** P1 · **Priority:** p1-high · **Effort:** M
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding backend-usage-script-no-limits. The critique moved this into P1. QuickJS runs with no interrupt handler, memory limit or stack limit, synchronously inside an async fn (evaluated twice). timeout_secs only bounds the HTTP call. `while(true){}` pins a worker for good, and each auto-query pins another until the runtime starves, which also takes down the proxy. Such scripts can arrive by deeplink.

## Evidence
- src-tauri/src/usage_script.rs:9-17, 34-52, 119-135, ~165; services/provider/usage.rs:13/22; commands/provider.rs:400; src/lib/query/queries.ts:250-264 (auto refetch)

## Proposed change
- For each Runtime, call set_memory_limit (32 MB), set_max_stack_size (512 KB), and set_interrupt_handler with a deadline (for example 5 s).
- Run both JS phases inside spawn_blocking and wrap them in tokio::time::timeout.
- Return a localized "script timed out" error.
- Optional (BE idea 1): a per-script host allowlist and a dry-run request view.

## Acceptance criteria / verification
- [ ] Tests, each under a 10 s test timeout: `while(true){}` returns Err near the deadline; `'a'.repeat(1e9)` returns Err; infinite recursion returns Err with no native stack overflow
- [ ] With several looping scripts on a 2-worker runtime, an unrelated sleep task still completes
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml usage_script`

## Dependencies
None.
