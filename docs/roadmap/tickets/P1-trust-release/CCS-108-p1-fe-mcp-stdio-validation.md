# CCS-108: [mcp] Treat a missing type as stdio when validating MCP server specs, in the UI and the backend

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p3-low`, `area:mcp`

> **Ticket key:** `p1-fe-mcp-stdio-validation` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding frontend-mcp-stdio-default-validation. {"args":[]} or {} gets past both validators, and upsert_server saves before validating.

## Evidence
- src/components/mcp/McpFormModal.tsx:348-358; useMcpValidation.ts:75-77; src-tauri/src/mcp/validation.rs:15; services/mcp.rs:19-28

## Proposed change
- Use `(type ?? "stdio")` in both frontend checks.
- Call validate_server_spec at the start of McpService::upsert_server.

## Acceptance criteria / verification
- [ ] Vitest: validateJsonConfig('{"args":[]}') gives commandRequired; submit is blocked
- [ ] Rust test: upsert_server({"args":[]}) returns Err and the DB is unchanged
- [ ] `pnpm test:unit` and `cargo test --manifest-path src-tauri/Cargo.toml mcp`

## Dependencies
None.
