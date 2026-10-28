# CCS-189: [provider-form] Add per-app, per-preset settingsConfig golden snapshots

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:F`, `p1-high`, `area:frontend-tests`

> **Ticket key:** `f-settingsconfig-goldens` · **Phase:** F · **Priority:** p1-high · **Effort:** S · **Depends on:** `p0-hotfix-providerform-parse`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
The ProviderForm split is high-risk, and nothing pins the produced settingsConfig.

## Proposed change
For each app and each preset, render the form, submit the defaults, and snapshot the onSubmit payload. Add edit-mode round-trip cases.

## Acceptance criteria / verification
- [ ] Snapshots exist for all presets of all 8 apps
- [ ] `pnpm test:unit`

## Dependencies
p0-hotfix-providerform-parse
