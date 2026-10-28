# CCS-163: [proxy] Support proxy takeover, failover and usage for OpenCode, OpenClaw and Hermes

**Status:** Open · **Kind:** feature · **Labels:** `feature`, `phase:P4`, `p2-medium`, `area:proxy`

> **Ticket key:** `p4-takeover-additive-apps` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p4-takeover-engine`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Wire routing already exists for these apps, but takeover is hard-coded false for opencode and openclaw (services/proxy.rs:714-716), and the status has no hermes field.

## Proposed change
Implement ProxyTakeover for the additive adapters, rewriting only the selected entry's baseUrl, and include them in failover and usage.

## Acceptance criteria / verification
- [ ] Crash/restore E2E is green for opencode, openclaw and hermes
- [ ] Conformance takeover checks pass
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml takeover` and `pnpm test:unit`

## Dependencies
p4-takeover-engine
