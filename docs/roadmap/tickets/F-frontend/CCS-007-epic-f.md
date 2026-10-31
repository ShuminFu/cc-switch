# CCS-007: [epic] Track F: Frontend decomposition (runs in parallel from P2 bindings, lands by v4.0)

**Status:** Partial · **Kind:** epic · **Labels:** `epic`, `phase:F`, `p2-medium`, `area:frontend`

> **Ticket key:** `epic-f` · **Phase:** F · **Priority:** p2-medium · **Effort:** XL · **Depends on:** `epic-p2`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Goal
Break up ProviderForm.tsx (2578 lines) and App.tsx, centralise query keys and invalidation, lazy-load views and locales, type the i18n keys, and add a tauri-driver smoke test that gates releases. Fix the frontend bugs whose structural fix is in this track.

## Exit criteria
- [x] ProviderForm.tsx and App.tsx are each 300 lines or fewer — `src/components/providers/forms/ProviderForm.tsx` is 53 lines and `src/App.tsx` is 296 lines (`wc -l`). CCS-250 (`b09df83`) moved the command palette and switching-state wiring out of `App.tsx`; before that it was 373.
- [x] App-literal comparisons in src/ are 40 or fewer (ESLint error) — `cc-switch/no-app-literal-comparison` is `error` in `eslint.config.js:66` and `pnpm lint` (`--max-warnings 0`) passes. `eslint-suppressions.json` holds 27 suppressions for the rule, and `tests/lint/eslintConfig.test.ts` pins `APP_LITERAL_BUDGET = 27` and asserts it is at most 40. CCS-251 took the count from 142 to 60 and CCS-252 from 60 to 27.
- [x] Per-app/per-preset settingsConfig goldens unchanged — `tests/components/providerFormGoldens` passes (part of the 22-file / 567-test targeted run below). The only snapshot edit since CCS-189 (`c080308`, CCS-199) changed two blocked-submit error strings to i18n keys, not any `settingsConfig` payload.
- [x] Initial JS bundle at least 30% smaller than the v3.17 baseline — re-measured after the follow-ups with `vite build` into a scratch dir: the only script in `index.html` is 972.12 kB (gzip 319.19 kB), vs 4,229.39 kB for v3.17.0 (CCS-198): −77.0 %.
- [x] Zero inline defaultValue fallbacks; validation messages localized in all 4 locales — `tests/i18n` (incl. `noInlineFallbacks`, parity, typed keys) and `tests/lib/validationMessagesI18n.test.ts` pass. Together with `tests/lint`, the goldens and `tests/ci/e2eSmoke.test.ts`: 22 files / 567 tests pass on vitest 4.1.12. The remaining `defaultValue` hits in `src/` are preset template defaults and Radix `Tabs`/`Accordion` props, not `t()` fallbacks. CCS-193 is Done; CCS-253 removed the last (dead) Chinese usage-script matchers.
- [ ] tauri-driver smoke green on the Linux AppImage and blocking release — **not met (needs CI + GitHub settings)**: the wiring is in place. `ci.yml:315` runs the smoke on the debug build in `tauri-build-smoke`, `release.yml:355` runs it on the AppImage before Prepare Linux Assets/upload, and `tests/ci/e2eSmoke.test.ts` passes. Locally it passed on the debug binary (30/30) and the AppImage (5/5) (CCS-201). No green GitHub Actions run has been observed, and it is not yet a required check.

## Child tickets
f-settingsconfig-goldens, f-providerform-shell, f-providerform-core-apps, f-providerform-additive-apps, f-i18n-validation-messages, f-query-keys, f-mutation-appid-variables, f-mutation-live-ids, f-app-shell-split, f-lazy-views-locales, f-typed-i18n, f-radix-dialog-component-tests, f-tauri-driver-e2e, f-resize-listener-leak, f-safe-storage, f-a11y-icon-buttons, f-live-diff-preview, f-unsaved-changes-guard, f-switching-state, f-command-palette, f-unpriced-requests-export

## Epic closure audit

Re-audited 2026-10-31 after the follow-up wave, against branch `claude/funny-carson-9c7j3e` (HEAD `673be73`, which merges the lane 2 follow-ups CCS-213..253).

| Ticket | Key | Status |
|---|---|---|
| CCS-189 | f-settingsconfig-goldens | Done |
| CCS-190 | f-providerform-shell | Done |
| CCS-191 | f-providerform-core-apps | Done |
| CCS-192 | f-providerform-additive-apps | Done |
| CCS-193 | f-i18n-validation-messages | Done |
| CCS-194 | f-query-keys | Done |
| CCS-195 | f-mutation-appid-variables | Done |
| CCS-196 | f-mutation-live-ids | Done |
| CCS-197 | f-app-shell-split | Done |
| CCS-198 | f-lazy-views-locales | Done |
| CCS-199 | f-typed-i18n | Done |
| CCS-200 | f-radix-dialog-component-tests | Done |
| CCS-201 | f-tauri-driver-e2e | Partial (needs CI) |
| CCS-202 | f-resize-listener-leak | Done |
| CCS-203 | f-safe-storage | Done |
| CCS-204 | f-a11y-icon-buttons | Done |
| CCS-205 | f-live-diff-preview | Done |
| CCS-206 | f-unsaved-changes-guard | Done |
| CCS-207 | f-switching-state | Done |
| CCS-208 | f-command-palette | Done |
| CCS-209 | f-unpriced-requests-export | Done |
| CCS-210 | f-markdowneditor-stale-onchange (added) | Done |
| CCS-211 | f-lint-green (added) | Done |
| CCS-250 | f-app-shell-under-300 (follow-up) | Done |
| CCS-251 | f-lint-app-literals-batch1 (follow-up) | Done |
| CCS-252 | f-lint-app-literals-batch2 (follow-up) | Done |
| CCS-253 | f-i18n-usage-cjk-matchers (follow-up) | Done |

Related follow-ups in other tracks that touch the frontend, all Done: CCS-216 (vitest upgrade), CCS-225 (settings load-failure notice), CCS-226 (e2e CSP/icons runtime check), CCS-229 (frontend app registry derived from the backend), CCS-237/238 (sync key UI and unsigned-migration prompt).

**Done.**
- `ProviderForm.tsx` is 53 lines (from 2578) and `App.tsx` is 296 lines (CCS-250).
- App-literal comparisons are down to 27 suppressions, under the 40 budget, with the rule at `error` and the budget pinned by a test (CCS-251, CCS-252).
- Query keys and invalidation are centralised; views, dialogs and locales are lazy-loaded; the entry chunk is 77.0 % smaller than v3.17.
- i18n keys are typed, inline fallbacks are gone, and validation messages are localized in en, zh, zh-TW and ja. CCS-193 is closed by CCS-253.
- The settingsConfig goldens pass unchanged and `pnpm lint` is green.
- The tauri-driver smoke is written, passes locally on the debug binary and the AppImage, and is wired into `ci.yml` and the Linux `release.yml` legs.

**Remaining (why Partial).** Only one criterion is open, and it can't be closed from this environment:
1. **tauri-driver smoke green in CI and blocking release (CCS-201).** Needs a green GitHub Actions run of `tauri-build-smoke` (`ubuntu-22.04`) and of the AppImage smoke in the Linux `release.yml` legs (`ubuntu-22.04`, `ubuntu-22.04-arm`, i.e. a release tag run). Then a maintainer has to add `tauri-build-smoke` as a required check in GitHub branch protection. After both, tick the criterion, set CCS-201 to Done and this epic to Done.
