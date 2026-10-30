# CCS-151: [mcp] Add McpProjector and generic MCP import loops

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p2-medium`, `area:mcp`

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
- [x] tests/mcp_commands.rs (19 tests) passes
- [x] The JSON-legacy migration fixture still imports MCP servers
- [x] `cargo test --manifest-path src-tauri/Cargo.toml mcp`

## Dependencies
p4-adapter-trait-gemini-grok, p0-migration-harness

## Implementation notes

**Summary.** `McpProjector` (from CCS-146) gains `read_servers()`, which returns an app's live MCP servers already converted to the unified JSON spec (not yet validated). The shared logic moved to the new `src-tauri/src/mcp/common.rs`:
- `collect_enabled_servers`: the single copy, used by the Claude, Codex and Gemini `sync_enabled_to_*` functions.
- `valid_imports`: validates each entry, then logs and skips the ones that fail.
- `imported_server`: builds a new server with only the importing app enabled.
- `import_into_config`: the `MultiAppConfig` merge.

Each `mcp/<app>.rs` now keeps only its reader (`read_servers_from_<app>`). The six copy-pasted `McpService::import_from_<app>` functions are replaced by one `McpService::import_from_app(state, app)`. `import_from_all_apps` and the startup "MCP table empty" import in `lib.rs` both loop over `registry::with_caps(AppCaps::MCP)`.

**Legacy JSON path.** `MultiAppConfig` stays as a shim. `migrate_from_json` is unchanged. The public `import_from_claude/codex/gemini/grokbuild(&mut MultiAppConfig)` functions still exist as one-line wrappers over `import_into_config`, because the lib re-exports them and `tests/import_export_sync.rs` uses them. The unused Hermes and OpenCode `MultiAppConfig` importers were removed. The JSON-legacy migration fixtures (`tests/fixtures/legacy/config-v3.6-per-app-mcp.json` and `config-v3.7-unified-mcp.json`, pinned in `tests/migrations.rs`) still import their MCP servers.

**Behaviour fix found during the refactor.** In a Codex `config.toml`, an entry with an unknown `type` used to stop the import of every entry after it in the same table, because the closure returned early. Now only that one entry is skipped. Regression test: `import_from_codex_skips_unknown_type_without_dropping_later_servers`. The old code would have returned 0 changed instead of 1. This was worked out from the code, not by running the test against the old version.

**Files.**
- New: `src-tauri/src/mcp/common.rs`
- `src-tauri/src/mcp/{mod,claude,codex,gemini,grokbuild,hermes,opencode}.rs`
- `src-tauri/src/apps/adapter.rs`
- `src-tauri/src/apps/{claude,codex,gemini,grok,hermes,opencode}/mod.rs`
- `src-tauri/src/services/mcp.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/tests/mcp_commands.rs`: calls moved to `import_from_app`; new test `import_from_all_apps_loops_over_every_mcp_capable_app` covers Claude, Grok Build and Hermes, OpenClaw returning 0, and an idempotent re-import.
- `src-tauri/tests/import_export_sync.rs`

**Commits.** `799a489` (code and tests); this ticket edit is in a follow-up `docs(roadmap)` commit.

**Verification.**
- `cargo fmt --check`: clean.
- `cargo clippy --all-targets -- -D warnings`: clean.
- `cargo test --test mcp_commands`: 21 passed (the original 20 tests plus the new one).
- `cargo test --test import_export_sync`: 27 passed.
- `cargo test --test migrations`: 13 passed, including the v3.6 and v3.7 legacy JSON MCP fixtures.
- `cargo test --test app_adapter_conformance`: 9 passed.
- `cargo test mcp`: 71 passed, 0 failed.
- `cargo test --no-fail-fast` (full suite): 2554 passed, 0 failed.
