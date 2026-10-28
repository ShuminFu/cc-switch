# CCS-082: [config] Route the remaining config writers through atomic_write and ban std::fs::write

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P1`, `p2-medium`, `area:config`

> **Ticket key:** `p1-route-writes-atomic-lint` · **Phase:** P1 · **Priority:** p2-medium · **Effort:** S · **Depends on:** `p1-atomic-write-perms`, `p0-config-writer-snapshots`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Several writers bypass atomic_write, so the README's atomic-write claim is false. The critique notes that a std::fs::write lint does not catch OpenOptions writers.

## Evidence
- claude_plugin.rs:83/117, services/env_manager.rs:139/220, commands/misc.rs:2773 (settings.rs is handled in p1-settings-atomic-save)

## Proposed change
- Switch these writers to config::atomic_write.
- Add src-tauri/clippy.toml with `disallowed-methods = ["std::fs::write"]`, with test modules exempt.
- Add a CI grep for `OpenOptions::new()` outside config.rs, the token stores and an allowlist.

## Acceptance criteria / verification
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` passes, with zero non-test std::fs::write
- [ ] P0 config-writer snapshots are unchanged

## Dependencies
p1-atomic-write-perms, p0-config-writer-snapshots
