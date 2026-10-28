# CCS-047: [docs] Update the package description to cover all supported tools

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p3-low`, `area:docs`

> **Ticket key:** `p0-docs-package-description` · **Phase:** P0 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-stale-description. The description lists only Claude Code, Codex and Gemini CLI, but 8 apps are supported.

## Evidence
- src-tauri/Cargo.toml:4, package.json:4; app_config.rs:28-35

## Proposed change
Use identical wording in both, for example "All-in-One Assistant for AI coding CLIs (Claude Code, Codex, Gemini CLI, OpenCode, and more)".

## Acceptance criteria / verification
- [ ] `grep -rn 'Codex & Gemini CLI' package.json src-tauri/Cargo.toml src-tauri/tauri.conf.json` finds nothing
- [ ] `cargo metadata --no-deps --manifest-path src-tauri/Cargo.toml` shows the new text

## Dependencies
None.
