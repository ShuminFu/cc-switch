# CCS-152: [takeover] Add crash/restore E2E for each proxy-capable app and a legacy-path runtime flag

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P4`, `p1-high`, `area:proxy-tests`

> **Ticket key:** `p4-takeover-crash-e2e` · **Phase:** P4 · **Priority:** p1-high · **Effort:** M · **Depends on:** `p0-proxy-harness-e2e`, `p0-config-writer-snapshots`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Takeover consolidation can corrupt users' settings.json, config.toml or auth.json. It needs a gate in place before any code moves.

## Proposed change
- For each of claude, codex, gemini and grokbuild: apply takeover, kill the process (simulated drop without cleanup), restart, and assert the live files are restored byte-for-byte.
- Add a `proxy.takeoverEngine=legacy` runtime flag for one release.

## Acceptance criteria / verification
- [ ] Green on 3 OSes
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test takeover_crash`

## Dependencies
p0-proxy-harness-e2e, p0-config-writer-snapshots
