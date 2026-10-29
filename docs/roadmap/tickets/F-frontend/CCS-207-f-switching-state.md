# CCS-207: [providers] Show a switching state and disable the other Switch buttons while a switch is pending

**Status:** Done · **Kind:** feature · **Labels:** `feature`, `phase:F`, `p3-low`, `area:frontend`

> **Ticket key:** `f-switching-state` · **Phase:** F · **Priority:** p3-low · **Effort:** S · **Depends on:** `f-mutation-appid-variables`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Frontend idea 2. There is no feedback during a switch, and double switches are possible.

## Proposed change
Show a per-card pending indicator from the mutation variables, and disable Switch on the other cards while it is pending.

## Acceptance criteria / verification
- [x] RTL with a deferred switch: the other buttons are disabled until it resolves
- [x] `pnpm test:unit`

## Dependencies
f-mutation-appid-variables

## Implementation notes

**Summary.** `useProviderActions` now exposes `switchingProviderId`, taken from
the switch mutation's variables while it is pending and only when
`variables.appId` matches the active app (builds on CCS-195). It flows
`App` → `ProvidersHome` → `ProviderList` → `ProviderCard` → `ProviderActions`
as `isSwitching` (this card) and `isSwitchPending` (any card). While a switch
is pending, every main action that would call `onSwitch` (normal Enable, OMO
enable, additive add-to-config, and Hermes' Enable button) is disabled; the
switching card shows a spinner and `aria-busy="true"`. Actions that do not
switch (failover queue toggle, remove-from-config) stay enabled.
`switchProvider` also returns early if a switch for the same app is already
in flight, so a double switch can't start even if a click gets through. No
new strings were added; the button keeps its label and swaps its icon.

**Files.**
- `src/hooks/useProviderActions.ts`
- `src/App.tsx`, `src/components/app-shell/ProvidersHome.tsx`
- `src/components/providers/ProviderList.tsx`, `ProviderCard.tsx`, `ProviderActions.tsx`
- `tests/components/ProviderSwitchingState.test.tsx` (new)

**Commits.** `bc737cc` (feature + tests); this ticket-notes commit follows it.

**Verification.**
- `npx vitest run tests/components/ProviderSwitchingState.test.tsx`: 4 passed.
  The main RTL test renders the real `ProviderList`/`ProviderCard`/
  `ProviderActions` with the real `useProviderActions` and switch mutation.
  The MSW `switch_provider` handler is held on a deferred promise. Other
  cards stay disabled and the switching card is busy until it resolves; a
  second click starts no second request. With the disable logic and the
  re-entrancy guard turned off, this test fails (`toBeDisabled`), so it is a
  real regression test.
- `pnpm typecheck`: clean.
- `pnpm test:unit`: 121 test files passed.
- `pnpm format:check`: all files formatted.

**Review.** I checked the change against the code. `ProvidersHome` passes
`switchingProviderId` on to `ProviderList` through `...listProps`. Every
`onSwitch` path in `ProviderActions` (normal, OMO, additive add, Hermes
Enable) is disabled while a switch is pending, and the actions that do not
switch stay enabled. One gap: in the RTL test the second click lands on a
disabled button, so React never runs the handler, and the re-entrancy guard
in `switchProvider` was not tested on its own. I added two hook tests in
`tests/hooks/useProviderActions.test.tsx`. One checks that a pending switch
for the same app blocks a second `mutateAsync` (it fails with the guard
removed). The other checks that a pending switch for another app does not
set `switchingProviderId` or block a switch. Re-verified: `pnpm typecheck`
clean, `pnpm test:unit` all files passed, `pnpm format:check` clean.
