# CCS-007: [epic] Track F: Frontend decomposition (runs in parallel from P2 bindings, lands by v4.0)

**Status:** Partial · **Kind:** epic · **Labels:** `epic`, `phase:F`, `p2-medium`, `area:frontend`

> **Ticket key:** `epic-f` · **Phase:** F · **Priority:** p2-medium · **Effort:** XL · **Depends on:** `epic-p2`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Goal
Break up ProviderForm.tsx (2578 lines) and App.tsx, centralise query keys and invalidation, lazy-load views and locales, type the i18n keys, and add a tauri-driver smoke test that gates releases. Fix the frontend bugs whose structural fix is in this track.

## Exit criteria
- [ ] ProviderForm.tsx and App.tsx are each 300 lines or fewer — **not met**: `ProviderForm.tsx` is 53 lines (met), but `src/App.tsx` is 373 lines. CCS-197 left it at 290; CCS-198 (306), CCS-207 (308), CCS-208 command palette (369/371) and CCS-211 (373) grew it back past 300.
- [ ] App-literal comparisons in src/ are 40 or fewer (ESLint error) — **not met**: the rule `cc-switch/no-app-literal-comparison` is `error` in `eslint.config.js` and `pnpm lint` passes with 0 warnings, but 142 hits are still suppressed in `eslint-suppressions.json` (`APP_LITERAL_BUDGET = 142` in `tests/lint/eslintConfig.test.ts`), well above 40.
- [x] Per-app/per-preset settingsConfig goldens unchanged — `CI=true npx vitest run tests/components/providerFormGoldens`: 8 files, 467 tests pass. The only snapshot edit since CCS-189 (`c080308`, CCS-199) changed two blocked-submit error strings to i18n keys, not any `settingsConfig` payload.
- [x] Initial JS bundle at least 30% smaller than the v3.17 baseline — re-measured with `vite build` into a scratch dir: the only script in `index.html` is 964.33 kB (gzip 315.33 kB), vs 4,229.39 kB for v3.17.0 (CCS-198): −77.2 %.
- [x] Zero inline defaultValue fallbacks; validation messages localized in all 4 locales — `tests/i18n` (incl. `noInlineFallbacks`, parity, typed keys) and `tests/lib/validationMessagesI18n.test.ts` pass (12 files / 95 tests together with `tests/lint`). The remaining `defaultValue` hits in `src/` are preset template defaults and Radix `Tabs`/`Accordion` props, not `t()` fallbacks. CCS-193 is Partial only because of Chinese matchers for backend/usage-script error text (`src/utils/errorUtils.ts`, `src/lib/query/queries.ts:161,163`). Those are not frontend validation messages.
- [ ] tauri-driver smoke green on the Linux AppImage and blocking release — **not met (needs CI)**: the wiring is in place. `ci.yml:284` runs the smoke in `tauri-build-smoke`, `release.yml:355` runs it on the AppImage before the upload, and `tests/ci/e2eSmoke.test.ts` passes (3 tests). Locally it passed on the debug binary and the AppImage (CCS-201). No green GitHub Actions run has been observed, and it is not yet a required check.

## Child tickets
f-settingsconfig-goldens, f-providerform-shell, f-providerform-core-apps, f-providerform-additive-apps, f-i18n-validation-messages, f-query-keys, f-mutation-appid-variables, f-mutation-live-ids, f-app-shell-split, f-lazy-views-locales, f-typed-i18n, f-radix-dialog-component-tests, f-tauri-driver-e2e, f-resize-listener-leak, f-safe-storage, f-a11y-icon-buttons, f-live-diff-preview, f-unsaved-changes-guard, f-switching-state, f-command-palette, f-unpriced-requests-export

## Epic closure audit

Audited 2026-10-30 against branch `claude/funny-carson-9c7j3e` (HEAD `c0c770a`).

| Ticket | Key | Status |
|---|---|---|
| CCS-189 | f-settingsconfig-goldens | Done |
| CCS-190 | f-providerform-shell | Done |
| CCS-191 | f-providerform-core-apps | Done |
| CCS-192 | f-providerform-additive-apps | Done |
| CCS-193 | f-i18n-validation-messages | Partial |
| CCS-194 | f-query-keys | Done |
| CCS-195 | f-mutation-appid-variables | Done |
| CCS-196 | f-mutation-live-ids | Done |
| CCS-197 | f-app-shell-split | Done |
| CCS-198 | f-lazy-views-locales | Done |
| CCS-199 | f-typed-i18n | Done |
| CCS-200 | f-radix-dialog-component-tests | Done |
| CCS-201 | f-tauri-driver-e2e | Partial |
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

**Done.**
- ProviderForm is decomposed. `ProviderForm.tsx` is 53 lines, down from 2578.
- The App shell is split, and query keys and invalidation are centralised.
- Views, dialogs and locales are lazy-loaded. The entry chunk is 77 % smaller than v3.17.
- i18n keys are typed and inline fallbacks are gone.
- Validation messages are localized in en, zh, zh-TW and ja.
- The settingsConfig goldens pass unchanged.
- The frontend bug and feature tickets are done.
- `pnpm lint` is green.
- The tauri-driver smoke is written and wired into `ci.yml` and the Linux `release.yml` legs.

**Remaining (why Partial).**
1. **`App.tsx` is 373 lines; the limit is 300.** CCS-198, 207, 208 and 211 added code after the CCS-197 split. Most of the growth is the CCS-208 command palette wiring (about 60 lines). To fix it, move the palette and the switching-state wiring out of `App.tsx` into `src/components/app-shell/` hooks or components. This needs a follow-up refactor ticket.
2. **App-literal comparisons: 142 suppressed; the target is 40 or fewer.** The rule is an ESLint `error` and the budget is ratcheted, but about 100 more literal comparisons still need to become registry capability lookups (`src/entities/app/registry.ts`). After each batch, run `pnpm lint:prune` and lower `APP_LITERAL_BUDGET`. No child ticket owns this reduction yet.
3. **tauri-driver smoke in CI (CCS-201).** This needs a green GitHub Actions run of `tauri-build-smoke` and of the Linux release legs (`ubuntu-22.04`, `ubuntu-22.04-arm`). It can't be verified from this environment. After that, a maintainer has to add the job as a required check in GitHub branch protection.
4. **CCS-193 (outside the epic criteria).** The Chinese matchers for backend and usage-script error text stay until the backend returns structured error codes (`IpcError`).

