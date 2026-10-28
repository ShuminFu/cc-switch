# CCS-018: [ci] Gate on dependency advisories with cargo-deny and pnpm audit

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p2-medium`, `area:ci`

> **Ticket key:** `p0-ci-cargo-deny` · **Phase:** P0 · **Priority:** p2-medium · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-no-audit: Dependabot opens version-bump PRs, but no CI step fails when a dependency has a known advisory.

## Evidence
- .github/workflows/ci.yml has no audit step; no deny.toml

## Proposed change
- Add src-tauri/deny.toml (advisories=deny, licenses, bans with duplicate warnings).
- Add EmbarkStudios/cargo-deny-action and `pnpm audit --audit-level=high` in CI.
- Add a weekly scheduled run.

## Acceptance criteria / verification
- [ ] Pinning a crate version with a known RUSTSEC advisory fails the job
- [ ] Local: `cargo deny --manifest-path src-tauri/Cargo.toml check`

## Dependencies
None.
