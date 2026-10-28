# CCS-146: [apps] Add the AppAdapter trait and migrate Gemini and Grok

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p1-high`, `area:apps`

> **Ticket key:** `p4-adapter-trait-gemini-grok` · **Phase:** P4 · **Priority:** p1-high · **Effort:** L · **Depends on:** `p2-app-registry`, `p1-route-writes-atomic-lint`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
About 74 match-app sites spread per-app logic across live.rs, prompt_files.rs, skill.rs, the commands and services/provider/mod.rs (3986 lines, which the critique notes the plan missed).

## Evidence
- live.rs:1016-1170 write_live_snapshot, :1309, :1427, :485-665; services/provider/mod.rs

## Proposed change
- Add apps/adapter.rs: trait AppAdapter (config_dir, prompt_file, skills_dir, read_live, write_provider, remove_provider, import_live, credentials, common_snippet, mcp(), takeover(), session_source()).
- Add apps/gemini/ and apps/grok/, with code moved from *_config.rs.
- Add settings::override_dir(app).

## Acceptance criteria / verification
- [ ] Gemini and Grok have no arms in live.rs or services/provider/mod.rs
- [ ] Writer snapshots are byte-identical
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test config_writers`

## Dependencies
p2-app-registry, p1-route-writes-atomic-lint
