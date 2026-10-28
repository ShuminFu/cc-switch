# CCS-147: [apps] Migrate Claude and Claude Desktop to AppAdapter

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p2-medium`, `area:apps`

> **Ticket key:** `p4-adapter-claude` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** L · **Depends on:** `p4-adapter-trait-gemini-grok`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
- Add apps/claude and apps/claude_desktop.
- Move settings.json, ~/.claude.json and the plugin writer.
- Remove their arms from live.rs, prompt_files.rs, skill.rs:500-550, commands/config.rs:90-180, commands/provider.rs:147-200 and services/provider/mod.rs.

## Acceptance criteria / verification
- [ ] Writer snapshots are identical
- [ ] The match-app count drops (track the number in the PR)
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml`

## Dependencies
p4-adapter-trait-gemini-grok
