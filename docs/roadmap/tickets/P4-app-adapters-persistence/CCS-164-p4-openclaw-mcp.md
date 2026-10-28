# CCS-164: [mcp] Add OpenClaw MCP support (Issue #4834)

**Status:** Open · **Kind:** feature · **Labels:** `feature`, `phase:P4`, `p3-low`, `area:mcp`

> **Ticket key:** `p4-openclaw-mcp` · **Phase:** P4 · **Priority:** p3-low · **Effort:** S · **Depends on:** `p4-mcp-projector`, `p4-adapter-additive`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Evidence
- A stub at services/mcp.rs:142

## Proposed change
Implement McpProjector for OpenClaw and set caps.mcp.

## Acceptance criteria / verification
- [ ] Conformance MCP upsert and remove for OpenClaw
- [ ] Writer snapshot for the OpenClaw MCP section
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml mcp`

## Dependencies
p4-mcp-projector, p4-adapter-additive
