# CCS-108: [mcp] Treat a missing type as stdio when validating MCP server specs, in the UI and the backend

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p3-low`, `area:mcp`

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
- [x] Vitest: validateJsonConfig('{"args":[]}') gives commandRequired; submit is blocked
- [x] Rust test: upsert_server({"args":[]}) returns Err and the DB is unchanged
- [x] `pnpm test:unit` and `cargo test --manifest-path src-tauri/Cargo.toml mcp`

## Dependencies
None.

## Implementation notes

**Summary.** Both frontend checks now use `type ?? "stdio"`, so a spec with no `type` and no `command` (`{"args":[]}`, `{}`) is reported as `mcp.error.commandRequired` and submit is blocked. `McpService::upsert_server` now calls `validate_server_spec` first, before it reads or writes the DB. The backend validator already treated a missing type as stdio; it is now re-exported as `crate::mcp::validate_server_spec`. Because `upsert_server` is shared, the Tauri `upsert_mcp_server`/`add_mcp_server` commands and the deeplink import path also reject invalid specs before saving.

**Files.**
- `src/components/mcp/McpFormModal.tsx`: submit-time check defaults missing type to stdio
- `src/components/mcp/useMcpValidation.ts`: `validateJsonConfig` defaults missing type to stdio
- `src-tauri/src/services/mcp.rs`: validate spec at start of `upsert_server`
- `src-tauri/src/mcp/mod.rs`: re-export `validate_server_spec`
- `tests/hooks/useMcpValidation.test.tsx`, `tests/components/McpFormModal.test.tsx`, `src-tauri/tests/mcp_commands.rs`: regression tests

**Commits.** `45242d1` (code + tests); this ticket update in a follow-up commit.

**Verification.**
- New tests fail with the old source (2 vitest failures, and the Rust test panics at `expect_err`) and pass with the fix.
- `pnpm typecheck`: clean. `pnpm test:unit`: 147 files / 1445 tests passed. Prettier check on the touched files: clean.
- `cargo fmt --check`: clean. `cargo clippy -- -D warnings`: clean.
- `cargo test --test mcp_commands`: 20 passed, including `upsert_mcp_server_rejects_stdio_spec_without_command_before_saving` (Err `McpValidation` for `{"args":[]}`/`{}` on both existing and new ids; DB unchanged).
- `cargo test --no-fail-fast` (full suite): 2463 passed, 0 failed.
