# CCS-100: [usage] Run macOS keychain reads off the async workers, with a timeout

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p2-medium`, `area:usage`

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
