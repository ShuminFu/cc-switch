# CCS-067: [usage-script] Run usage scripts with interrupt, memory and stack limits off the tokio workers

**Status:** Done · **Kind:** security · **Labels:** `security`, `phase:P1`, `p1-high`, `area:usage-script`

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
- [x] Tests, each under a 10 s test timeout: `while(true){}` returns Err near the deadline; `'a'.repeat(1e9)` returns Err; infinite recursion returns Err with no native stack overflow
- [x] With several looping scripts on a 2-worker runtime, an unrelated sleep task still completes
- [x] `cargo test --manifest-path src-tauri/Cargo.toml usage_script`

## Dependencies
None.

## Implementation notes

**Summary.** `src-tauri/src/usage_script.rs` gained a `run_sandboxed` helper, and both JS phases go through it: the config/request parse and the extractor call. For each phase it:
- runs the QuickJS `Runtime`/`Context` inside `tokio::task::spawn_blocking`, so a looping script never holds a tokio worker;
- sets `set_memory_limit(32 MB)`, `set_max_stack_size(512 KB)` and an `set_interrupt_handler` that fires at a 5 s deadline (`JS_EXEC_TIMEOUT`);
- wraps the blocking task in `tokio::time::timeout(5 s + 1 s grace)` as a backstop for a single long native operation;
- turns an interrupted run (or an outer timeout) into the localized error `usage_script.timed_out` ("脚本执行超时（超过 5 秒）" / "Script timed out (exceeded 5s)"). It is not a transient key, so `services/provider/usage.rs` shows it as `success:false` and does not retry it.

The HTTP request still runs async between the two phases. The optional host allowlist and dry-run view (BE idea 1) were not done.

**Files.** `src-tauri/src/usage_script.rs` (implementation + tests).

**Commits.** `60505ce` fix(usage-script): sandbox QuickJS with interrupt, memory and stack limits (CCS-067), plus this ticket-notes commit.

**Tests** (in `usage_script::tests`; each runs on its own thread with a 10 s hard timeout through `recv_timeout`):
- `usage_script_infinite_loop_is_interrupted_near_deadline`: `while(true){}` gives `usage_script.timed_out` at about 5 s.
- `usage_script_huge_allocation_hits_memory_limit`: `'a'.repeat(1e9)` gives Err (`config_parse_failed`).
- `usage_script_infinite_recursion_errors_without_stack_overflow`: unbounded recursion gives Err, with no native stack overflow.
- `usage_script_extractor_phase_is_sandboxed`: a looping extractor is interrupted.
- `usage_script_sandbox_still_returns_normal_results`: a normal script still evaluates.
- `usage_script_looping_scripts_do_not_starve_runtime_workers`: 4 looping scripts on a 2-worker multi-thread runtime; an unrelated 100 ms sleep task still finishes in under 2 s, and all loops end with `timed_out`.

I confirmed the tests catch the bug by temporarily disabling the memory limit and the interrupt deadline. The loop, extractor, starvation and allocation tests then FAILED (4 failed). With the limits restored, all pass.

**Verification.**
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`: clean.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: clean. The same with `--lib --tests`: clean.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib usage_script`: 12 passed, 0 failed (5.0 s).
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: 2326 passed, 0 failed.
