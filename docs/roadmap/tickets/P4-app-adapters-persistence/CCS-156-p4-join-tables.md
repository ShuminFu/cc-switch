# CCS-156: [db] Replace per-app MCP and skill columns with join tables, dual-writing the old columns for one release

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p2-medium`, `area:database`

> **Ticket key:** `p4-join-tables` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** L · **Depends on:** `p4-migration-framework`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Evidence
- dao/mcp.rs, dao/skills.rs (per-app columns, positional row.get); the proxy_config CHECK constraint

## Proposed change
- Add a migration: mcp_server_apps and skill_apps join tables, and drop the proxy_config CHECK.
- The DAOs stop naming apps and use named columns.
- Keep writing the old columns for one release so mixed-version WebDAV/S3 sync works, and bump DB_COMPAT_VERSION per policy.
- AppSet replaces McpApps and SkillApps.

## Acceptance criteria / verification
- [ ] The migration harness passes; an older-version fixture importing the new dump keeps its app flags
- [ ] A test-only 9th app needs no migration
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test migrations dao::`

## Dependencies
p4-migration-framework
