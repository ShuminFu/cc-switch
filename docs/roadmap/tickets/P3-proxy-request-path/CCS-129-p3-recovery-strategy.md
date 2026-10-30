# CCS-129: [proxy] Replace nested forward() retries with a RecoveryStrategy list

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P3`, `p2-medium`, `area:proxy`

> **Ticket key:** `p3-recovery-strategy` · **Phase:** P3 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p3-attempt-recorder`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
The rectifiers retry by calling forward() again from inside it, so forward() has several call sites.

## Evidence
- forwarder.rs:579, 725, 891

## Proposed change
Add pipeline/recovery.rs with RecoveryStrategy {MediaDowngrade, ThinkingSignatureRectifier, ThinkingBudgetRectifier}. Each is applied at most once and is visible in the trace.

## Acceptance criteria / verification
- [x] forward() has exactly one call site
- [x] A proxy_e2e scenario for each rectifier passes
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_e2e`

## Dependencies
p3-attempt-recorder

## Implementation notes

**Summary.** New `src-tauri/src/proxy/pipeline/recovery.rs` defines `RecoveryStrategy`
(`MediaDowngrade`, `ThinkingSignatureRectifier`, `ThinkingBudgetRectifier`, checked in
`RecoveryStrategy::ORDER`) and a per-attempt `RecoveryState`. When an upstream call fails,
`RecoveryState::recover` returns one of three decisions. `Retry(strategy)` means the body was
rewritten in place and the same provider is called again. `ClientError` means a rectifier
recognised the error but can't fix it, or has already been applied. `Unhandled` means the
normal error categorisation runs. In `forward_with_retry_inner` the three nested
`forward()` retry blocks are now one inner `loop` around a single `forward()` call, so the
call site count dropped from 4 to 1 in production code. The only other call is the existing
`streaming_raw_path_uses_first_byte_timeout_for_headers` unit test.

Each strategy is applied at most once per provider attempt. Strategies can now chain: for
example, a media downgrade followed by a signature rectification. Before this change, a
failed media retry was returned as a client error. Every applied strategy is logged as
`[Recovery] provider=… strategy=… trace=[…]`, and a failed recovery also logs
`[Recovery] … retry failed after trace=[…]`. The existing RECT-001/002/003/005/006/010/011/
012/013/014 and `[Media]` log lines are kept. If a recovery retry still fails, the error
goes through `handle_rectifier_retry_failure` as before: provider errors fail over and
client errors are returned. The reactive media gate moved to
`recovery::media_downgrade_should_trigger`, and `extract_error_message` moved into
recovery.rs.

The plan's per-request trace inspector is not built yet. For now the trace is only visible
in the logs. `ForwardResult` has no recoveries field because nothing would read it yet.

**Files.**
- `src-tauri/src/proxy/pipeline/recovery.rs` (new, with unit tests)
- `src-tauri/src/proxy/pipeline/mod.rs`
- `src-tauri/src/proxy/forwarder.rs`
- `src-tauri/tests/proxy_e2e.rs`: new scenarios `recovery_strategies_chain_on_the_same_provider`
  (a regression test: with the old code it returned 400 after 2 requests) and
  `recovery_strategy_is_applied_at_most_once`. The existing signature, budget and media
  rectifier scenarios still pass.

**Commits.** 7b1aecb (code). The ticket-notes commit follows it.

**Verification.**
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: clean
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`: clean
- `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_e2e`: 21 passed, 0 failed
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: lib 2261 passed, 0 failed,
  3 ignored. All integration test binaries passed.
