# CCS-179: [commands] Split misc.rs into cli_tools services and a unified platform terminal stack

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P5`, `p2-medium`, `area:commands`

> **Ticket key:** `p5-misc-split-terminal` · **Phase:** P5 · **Priority:** p2-medium · **Effort:** L · **Depends on:** `p1-powershell-quote`, `p0-hotfix-terminal-tempfiles`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Evidence
- commands/misc.rs (~3.5k lines: search paths, version probes, installs, lifecycle, terminals); session_manager/terminal

## Proposed change
- services/cli_tools/{search_paths,version_probe,installations,lifecycle_cmd,remote_versions}.rs.
- platform/terminal/{macos,linux,windows}.rs, merged with session_manager/terminal and keeping the P0/P1 tempfile and quoting fixes.

## Acceptance criteria / verification
- [ ] commands/misc.rs is deleted
- [ ] The terminal unit tests (tempfile paths, powershell quoting) pass on 3 OSes
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml terminal cli_tools`

## Dependencies
p1-powershell-quote, p0-hotfix-terminal-tempfiles
