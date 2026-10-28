# CCS-134: [usage] Add a canonical UsageIR with from/to converters for each format

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P3`, `p1-high`, `area:proxy-usage`

> **Ticket key:** `p3-usage-ir` · **Phase:** P3 · **Priority:** p1-high · **Effort:** M · **Depends on:** `p0-golden-responses-fixtures`, `p1-usage-input-semantics`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Cache-token arithmetic is reimplemented in every bridge, and the copies drift.

## Evidence
- transform_responses.rs:431, transform_codex_chat.rs:1672, transform_codex_anthropic.rs:144, transform_gemini.rs:1101; UsageParser::from_*

## Proposed change
Add proxy/usage/ir.rs: UsageIR {fresh_input, cache_read, cache_write, output, reasoning, semantics}, with from_<format>/to_<format>. The bridges and UsageParser delegate to it. Add proptest for billable-token conservation.

## Acceptance criteria / verification
- [ ] `grep -rn 'cached_tokens' src-tauri/src/proxy/providers` shows only UsageIR call sites
- [ ] Golden usage.expected is identical, and the proptest passes
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml usage::ir`

## Dependencies
p0-golden-responses-fixtures, p1-usage-input-semantics
