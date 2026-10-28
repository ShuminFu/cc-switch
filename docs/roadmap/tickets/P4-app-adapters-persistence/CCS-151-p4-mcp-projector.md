# CCS-151: [mcp] Add McpProjector and generic MCP import loops

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p2-medium`, `area:mcp`

> **Ticket key:** `p4-mcp-projector` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p4-adapter-trait-gemini-grok`, `p0-migration-harness`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
collect_enabled_servers is duplicated, and the import paths are about 300 copy-pasted lines. Per the critique, dropping MultiAppConfig could break the legacy JSON upgrade path, which must be pinned first.

## Evidence
- mcp/claude.rs:18, codex.rs:23, gemini.rs:18; services/mcp.rs:294-460; lib.rs:788-815

## Proposed change
- Add mcp/common.rs with collect_enabled_servers and one import_from_app.
- Turn lib.rs startup into a loop over APPS with caps.mcp.
- Keep a MultiAppConfig shim for migrate_from_json only.

## Acceptance criteria / verification
- [ ] tests/mcp_commands.rs (19 tests) passes
- [ ] The JSON-legacy migration fixture still imports MCP servers
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml mcp`

## Dependencies
p4-adapter-trait-gemini-grok, p0-migration-harness
