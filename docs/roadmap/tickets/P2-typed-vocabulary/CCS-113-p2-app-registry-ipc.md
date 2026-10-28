# CCS-113: [apps] Expose get_app_registry, add the exhaustiveness test, and fix the prompt and skills-dir edge cases

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P2`, `p2-medium`, `area:apps`

> **Ticket key:** `p2-app-registry-ipc` · **Phase:** P2 · **Priority:** p2-medium · **Effort:** S · **Depends on:** `p2-app-registry`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
The frontend duplicates capability knowledge. prompt_files.rs has an unreachable!() for some apps, and Claude Desktop reports a bogus skills dir.

## Evidence
- prompt_files.rs:31/38 unreachable!; skill.rs:544

## Proposed change
- Add a get_app_registry command that returns the descriptors.
- Add an exhaustiveness test: every variant has a descriptor, an i18n label in all locales, an icon and a presets file.
- Return a typed Unsupported error instead of panicking, and return None for Claude Desktop skills.

## Acceptance criteria / verification
- [ ] The exhaustiveness test fails when a dummy variant lacks a label
- [ ] Test: the prompt path for each app is Ok or Unsupported, with no panic
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml apps::`

## Dependencies
p2-app-registry
