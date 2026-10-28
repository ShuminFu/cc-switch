# CCS-007: [epic] Track F: Frontend decomposition (runs in parallel from P2 bindings, lands by v4.0)

**Status:** Open · **Kind:** epic · **Labels:** `epic`, `phase:F`, `p2-medium`, `area:frontend`

> **Ticket key:** `epic-f` · **Phase:** F · **Priority:** p2-medium · **Effort:** XL · **Depends on:** `epic-p2`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Goal
Break up ProviderForm.tsx (2578 lines) and App.tsx, centralise query keys and invalidation, lazy-load views and locales, type the i18n keys, and add a tauri-driver smoke test that gates releases. Fix the frontend bugs whose structural fix is in this track.

## Exit criteria
- [ ] ProviderForm.tsx and App.tsx are each 300 lines or fewer
- [ ] App-literal comparisons in src/ are 40 or fewer (ESLint error)
- [ ] Per-app/per-preset settingsConfig goldens unchanged
- [ ] Initial JS bundle at least 30% smaller than the v3.17 baseline
- [ ] Zero inline defaultValue fallbacks; validation messages localized in all 4 locales
- [ ] tauri-driver smoke green on the Linux AppImage and blocking release

## Child tickets
f-settingsconfig-goldens, f-providerform-shell, f-providerform-core-apps, f-providerform-additive-apps, f-i18n-validation-messages, f-query-keys, f-mutation-appid-variables, f-mutation-live-ids, f-app-shell-split, f-lazy-views-locales, f-typed-i18n, f-radix-dialog-component-tests, f-tauri-driver-e2e, f-resize-listener-leak, f-safe-storage, f-a11y-icon-buttons, f-live-diff-preview, f-unsaved-changes-guard, f-switching-state, f-command-palette, f-unpriced-requests-export
