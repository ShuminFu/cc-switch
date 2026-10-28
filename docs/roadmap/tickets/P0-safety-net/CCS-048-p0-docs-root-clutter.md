# CCS-048: [docs] Move the session-manager PRD into docs/ and leave the Pages-served deplink.html alone

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p3-low`, `area:docs`

> **Ticket key:** `p0-docs-root-clutter` · **Phase:** P0 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-root-clutter, partially real. session-manager.md (268-line PRD) sits at the repo root and nothing references it. deplink.html is served by GitHub Pages and linked from 6 user-manual pages, so it must stay.

## Evidence
- /session-manager.md; docs/user-manual/{en,ja,zh}/2-providers/2.1-add.md:352 and 5-faq/5.3-deeplink.md:16 link deplink.html

## Proposed change
Move session-manager.md to docs/roadmap/. Add a comment at the top of deplink.html saying it is served by Pages.

## Acceptance criteria / verification
- [ ] `grep -rn session-manager.md . --exclude-dir=node_modules` finds no broken reference
- [ ] https://farion1231.github.io/cc-switch/deplink.html still loads

## Dependencies
None.
