# CCS-012: [apps] Add round-trip snapshots for all live-config writers, OAuth token stores and the Codex state DB

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p1-high`, `area:apps-tests`

> **Ticket key:** `p0-config-writer-snapshots` · **Phase:** P0 · **Priority:** p1-high · **Effort:** M
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Takeover and adapter refactors will rewrite users' CLI configs, and nothing pins byte-stability today. The critique adds two gaps: the OAuth token stores (own atomic writers) and the Codex state DB written by codex_history_migration.rs.

## Evidence
- Writers: claude settings.json, ~/.claude.json MCP (claude_mcp.rs:345), codex config.toml/auth.json (codex_config.rs:244), gemini .env/settings.json, grok, opencode, openclaw, hermes, claude_desktop, claude_plugin.rs:83/117
- proxy/providers/copilot_auth.rs:1247-1294 and codex_oauth_auth.rs (OpenOptions writers)
- codex_history_migration.rs:644,1157 updates threads.model_provider using CC_SWITCH_CODEX_MODEL_PROVIDER_ID
- Existing pattern: tests/hermes_roundtrip.rs

## Proposed change
- Add tests/config_writers.rs with tests/fixtures/live/** inputs that include comments, unknown keys, CRLF and a BOM.
- Snapshot the output, and assert that untouched sections are byte-identical.
- Snapshot the token-store JSON shape and file mode.
- Snapshot the model_provider id and the settings.json migration markers.

## Acceptance criteria / verification
- [ ] 11 writers + 2 token stores + the Codex state DB covered
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test config_writers`

## Dependencies
None (p0-testenv-paths makes it hermetic later).
