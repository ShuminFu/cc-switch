# CCS-066: [bridge] Decide Gemini cumulative vs incremental mode once per stream

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p3-low`, `area:proxy-bridge`

> **Ticket key:** `p1-bridge-gemini-delta` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S · **Depends on:** `p0-golden-testkit-chat-gemini`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding proxy-gemini-delta-drop. Classification runs on every chunk, so incremental chunks that start with all prior text ("ha" then "haha") lose text, and identical chunks are dropped.

## Evidence
- src-tauri/src/proxy/providers/streaming_gemini.rs:356-365, 394-398

## Proposed change
Track `mode: Option<bool>` per stream. Set it at the first unambiguous chunk and default to incremental.

## Acceptance criteria / verification
- [ ] Test: incremental "ha","haha","ha" gives "hahahaha"; cumulative "ha","haha","hahaha" gives "hahaha"
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml streaming_gemini`

## Dependencies
p0-golden-testkit-chat-gemini
