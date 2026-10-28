# CCS-078: [security] Remove the unused asset protocol and protocol-asset feature

**Status:** Open · **Kind:** security · **Labels:** `security`, `phase:P1`, `p3-low`, `area:security`

> **Ticket key:** `p1-asset-protocol-remove` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-unused-asset-protocol, partially real. The asset protocol is enabled with an empty scope and is never used. This is dead config.

## Evidence
- src-tauri/tauri.conf.json:30-33; src-tauri/Cargo.toml:30 protocol-asset; no convertFileSrc or asset:// anywhere in src/

## Proposed change
Remove both together, because tauri-build checks that they match.

## Acceptance criteria / verification
- [ ] `cargo check --manifest-path src-tauri/Cargo.toml` passes
- [ ] `grep -rn 'convertFileSrc\|asset://' src` finds nothing; icons still render in `pnpm tauri dev`

## Dependencies
None.
