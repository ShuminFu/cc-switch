# CCS-201: [e2e] Add a tauri-driver and WebdriverIO smoke on Linux that gates release

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:F`, `p2-medium`, `area:e2e`

> **Ticket key:** `f-tauri-driver-e2e` · **Phase:** F · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p0-ci-build-smoke`, `p0-ci-release-gated`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
An xvfb job that covers: first run, add a preset, switch and check ~/.claude/settings.json, then takeover on and restore. Run it on the debug build in PR CI and on the AppImage before upload.

## Acceptance criteria / verification
- [ ] The job is green in CI
- [ ] A failing smoke blocks release.yml upload

## Dependencies
p0-ci-build-smoke, p0-ci-release-gated
