# CCS-009: [proxy-tests] Add golden fixtures for the Responses bridges and recent regression commits

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p1-high`, `area:proxy-tests`

> **Ticket key:** `p0-golden-responses-fixtures` · **Phase:** P0 · **Priority:** p1-high · **Effort:** M · **Depends on:** `p0-golden-testkit-chat-gemini`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
The Responses bridges (anthropic<->responses, responses->chat) have had the most fix commits and are pinned only by inline tests.

## Evidence
- streaming_responses.rs, streaming_codex_chat.rs, streaming_codex_anthropic.rs inline concat!() cases
- Regression commits 6d316c0, 27ce0a5, 650905a, 613fef7, f991726

## Proposed change
- Using the testkit, add fixtures under anthropic__responses, responses__chat and responses__anthropic.
- Give each regression commit a case whose meta.toml cites the commit.

## Acceptance criteria / verification
- [ ] At least 30 new fixtures, for a total of 60 or more across all 5 bridges
- [ ] Every regression commit listed has a fixture that fails when its fix is reverted (verify by hand for at least 2)
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden`

## Dependencies
p0-golden-testkit-chat-gemini
