# CCS-005: [epic] P4 (v3.22): Per-app adapters, takeover engine, persistence and migration framework

**Status:** Open · **Kind:** epic · **Labels:** `epic`, `phase:P4`, `p1-high`, `area:apps`

> **Ticket key:** `epic-p4` · **Phase:** P4 · **Priority:** p1-high · **Effort:** XL · **Depends on:** `epic-p3`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Goal
Collapse the per-app fan-out (including services/provider/mod.rs, which the plan missed) and give usage, pricing and schema a single owner each, one app or table per PR. The versioned migration framework lands first, so no schema change ships the old way. Then ship takeover for additive apps, OpenClaw MCP and the related features.

## Exit criteria
- [ ] Migration framework: MIGRATIONS is contiguous and ends at SCHEMA_VERSION (CI check); create_tables is deleted; each schema bump has a documented DB_COMPAT_VERSION decision
- [ ] live.rs, prompt_files.rs, skill.rs and commands/config.rs contain no match app_type; backend match-app sites outside apps/** number 15 or fewer
- [ ] Conformance suite green for all 8 apps (credentials, MCP, prompt/skills paths, takeover apply/cleanup restores bytes, caps match traits, codex model_provider id stable)
- [ ] No services/proxy/ file exceeds 1500 lines of production code; crash/restore E2E green for claude, codex, gemini, grokbuild, opencode, openclaw and hermes
- [ ] Fresh and migrated DBs are schema-equivalent; a test-only 9th app touches 6 or fewer backend files and needs no migration
- [ ] One cost function; the cost golden table (at least 50 rows) passes; backfill output equals ingest output
- [ ] No SQL outside database/ (CI grep); dashboard summary p95 is 50 ms or less on a 1M-row DB; superseded_by backfill is chunked and resumable
- [ ] Killing the process mid-import causes no double counting; a downgraded app still reads current providers through dual-written keys

## Child tickets
p4-migration-framework, p4-adapter-trait-gemini-grok, p4-adapter-claude, p4-adapter-codex, p4-adapter-additive, p4-adapter-conformance, p4-mcp-projector, p4-takeover-crash-e2e, p4-services-proxy-split, p4-takeover-engine, p4-per-app-settings-state, p4-join-tables, p4-pricing-engine, p4-pricing-provenance, p4-usage-repo, p4-superseded-by-backfill, p4-rollup-retention, p4-json-key-order, p4-takeover-additive-apps, p4-openclaw-mcp, p4-capability-matrix, p4-drift-detection, p4-budget-alerts, p4-spend-cap-enforcement, p4-retention-panel, p4-live-config-journal, p4-usage-rescan, p4-keychain-secrets, p4-sync-snapshot-auth
