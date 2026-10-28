# CCS-204: [a11y] Give icon-only buttons accessible names

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:F`, `p3-low`, `area:a11y`

> **Ticket key:** `f-a11y-icon-buttons` · **Phase:** F · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding frontend-a11y-icon-buttons, partially real. About 11 unlabeled size="icon" buttons were confirmed. UniversalProviderCard is fine. The total of 61 is unverified.

## Evidence
- OpenCodeFormFields.tsx ~780,893,1030,1115; OmoFormFields.tsx:1028; BackupListSection.tsx:328,337; SessionManagerPage.tsx:825,999,1136,1591

## Proposed change
- Add aria-label={t(...)} to each unlabeled button. For tooltip-wrapped buttons, use the TooltipContent text.
- Add a dev warning in Button when size=icon has no label.

## Acceptance criteria / verification
- [ ] RTL getByRole('button',{name}) finds each button in BackupListSection rename mode and the SessionManagerPage toolbar
- [ ] jest-axe reports no button-name violations
- [ ] `pnpm test:unit`

## Dependencies
None.
