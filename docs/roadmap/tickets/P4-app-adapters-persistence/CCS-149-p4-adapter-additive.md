# CCS-149: [apps] Migrate OpenCode, OpenClaw and Hermes to AppAdapter and remove the remaining fan-out

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p2-medium`, `area:apps`

> **Ticket key:** `p4-adapter-additive` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** L · **Depends on:** `p4-adapter-claude`, `p4-adapter-codex`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
- Add apps/opencode (including OMO and OMO-Slim), apps/openclaw and apps/hermes.
- Replace the 7 get_<app>_override_dir functions with settings::override_dir.
- Remove the remaining match-app sites in services/provider/mod.rs.

## Acceptance criteria / verification
- [ ] live.rs, prompt_files.rs, skill.rs and commands/config.rs have no match app_type
- [ ] Backend match-app sites outside apps/** number 15 or fewer (CI count)
- [ ] Writer snapshots are identical

## Dependencies
p4-adapter-claude, p4-adapter-codex
