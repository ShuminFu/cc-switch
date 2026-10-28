# CCS-187: [proxy] Add the first new client/upstream combination: Gemini CLI to an Anthropic upstream

**Status:** Open · **Kind:** feature · **Labels:** `feature`, `phase:P5`, `p3-low`, `area:proxy`

> **Ticket key:** `p5-gemini-cli-anthropic` · **Phase:** P5 · **Priority:** p3-low · **Effort:** M · **Depends on:** `p5-codec-gemini`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
Add an endpoint plus a RoutePlan for a Gemini client to an Anthropic upstream, using the codecs.

## Acceptance criteria / verification
- [ ] At least 10 new golden fixtures, stream and non-stream
- [ ] proxy_e2e scenario
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test proxy_golden`

## Dependencies
p5-codec-gemini
