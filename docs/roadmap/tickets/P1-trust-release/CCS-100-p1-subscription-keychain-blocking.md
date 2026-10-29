# CCS-100: [usage] Run macOS keychain reads off the async workers, with a timeout

**Status:** Partial · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p2-medium`, `area:usage`

> **Ticket key:** `p1-subscription-keychain-blocking` · **Phase:** P1 · **Priority:** p2-medium · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding backend-subscription-security-blocking. get_subscription_quota runs `security find-generic-password` synchronously. A keychain prompt blocks a tokio worker until the user answers, and one worker is blocked per tool.

## Evidence
- src-tauri/src/services/subscription.rs:131-153, 497-515, 801-830, 1233; commands/subscription.rs:19-24; commands/provider.rs:634

## Proposed change
Use tokio::process::Command with kill_on_drop and tokio::time::timeout (20 s), falling back to the credentials file. Alternatively wrap the existing readers in spawn_blocking.

## Acceptance criteria / verification
- [ ] `grep -n 'std::process::Command::new("security")' src-tauri/src/services/subscription.rs` finds nothing
- [ ] macOS: `cargo clippy` on the macOS CI runner; manual check that other commands respond while a prompt is open
- [ ] Unit test with an injected slow reader on a current_thread runtime: a concurrent task makes progress

## Dependencies
None.

## Implementation notes

**Status rationale:** Partial — all code and Linux-verifiable items are done; the macOS-runner clippy run and the manual "other commands respond while a keychain prompt is open" check need a macOS machine and were not possible in this Linux environment.

**Summary.** The three keychain readers (Claude `Claude Code-credentials`, Codex `Codex Auth`, Gemini `gemini-cli-oauth`/`main-account`) no longer call `std::process::Command::new("security")`. They share `read_keychain_secret`, which runs `security find-generic-password … -w` via a new `run_secret_command` helper built on `tokio::process::Command` with `kill_on_drop(true)` and `tokio::time::timeout` (`KEYCHAIN_READ_TIMEOUT` = 20 s). Failure, empty output or timeout return `None`, so the caller falls back to the credentials file as before (a timeout is logged at warn level). `read_{claude,codex,gemini}_credentials` and the keychain readers are now `async` and awaited in `get_subscription_quota`. Enabled tokio's `process` feature (Cargo.lock gains `signal-hook-registry`).

**Files.**
- `src-tauri/src/services/subscription.rs`
- `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`

**Commits.** `5cdf262` fix(usage): read macOS keychain asynchronously with a timeout (CCS-100); this ticket-file update follows as a docs commit.

**Tests (inline, `services::subscription::tests`, unix, `current_thread` runtime):**
- `slow_secret_reader_does_not_block_concurrent_tasks` — injects a slow reader (`sh -c "sleep 1; printf …"`) while a ticker task runs every 10 ms; asserts the secret is returned trimmed and the ticker advanced ≥ 10 times.
- `slow_secret_reader_times_out` — `sleep 30` reader with a 200 ms timeout returns `None` in < 5 s.
- `secret_reader_failures_return_none` — non-zero exit, blank output and a missing binary all return `None`.

**Verification evidence.**
- `grep -n 'std::process::Command::new("security")' src-tauri/src/services/subscription.rs` → no matches.
- Fail-before: temporarily swapping the helper back to a blocking `std::process::Command` made both slow-reader tests fail ("got 0 ticks"; the timeout test took 30 s). With the fix they pass.
- `cargo fmt --check` → clean; `cargo clippy -- -D warnings` → clean on Linux.
- macOS code paths: `cargo clippy --lib -- -D warnings` also ran with the file's `target_os = "macos"` gates temporarily swapped to `unix`, which compiled the keychain branches on Linux → clean (then reverted).
- `cargo test --lib subscription` → 8 passed; full `cargo test --no-fail-fast` → 2442 passed / 0 failed.

**Remaining (needs macOS):** run `cargo clippy` on the macOS CI runner, and manually confirm other commands keep responding while a keychain prompt is open (and that the quota falls back to the file after 20 s if the prompt is ignored).
