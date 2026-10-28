# CCS-188: [cli-tools] Stream installer output and support cancel and timeout for tool lifecycle actions

**Status:** Open · **Kind:** feature · **Labels:** `feature`, `phase:P5`, `p3-low`, `area:cli-tools`

> **Ticket key:** `p5-tool-lifecycle-streaming` · **Phase:** P5 · **Priority:** p3-low · **Effort:** M · **Depends on:** `p5-misc-split-terminal`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Backend idea 9. run_tool_lifecycle_action blocks until the command ends, with no output and no limit.

## Proposed change
Spawn with tokio::process, emit line events, and add a cancel command and a configurable timeout.

## Acceptance criteria / verification
- [ ] Test with a fake installer script: lines arrive as events, cancel kills it, and a timeout returns an error
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml cli_tools` and `pnpm test:unit`

## Dependencies
p5-misc-split-terminal
