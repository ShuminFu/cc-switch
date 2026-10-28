# CCS-104: [terminal] Escape apostrophes in PowerShell launch paths

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p3-low`, `area:terminal`

> **Ticket key:** `p1-powershell-quote` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S · **Depends on:** `p0-hotfix-terminal-tempfiles`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding backend-powershell-path-escape. `& '<bat path>'` is built without escaping. A profile such as O'Brien fails to parse, and the cmd fallback never runs because `start` succeeds.

## Evidence
- src-tauri/src/commands/misc.rs:3245 and :3484 (fallbacks at 3258-3266, 3496-3504)

## Proposed change
Add `powershell_single_quote(s)`, which doubles each ', and use it at both sites.

## Acceptance criteria / verification
- [ ] Platform-independent unit test: `C:\Users\O'Brien\...\x.bat` gives `'C:\Users\O''Brien\...\x.bat'`
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml powershell_single_quote`

## Dependencies
p0-hotfix-terminal-tempfiles (same file)
