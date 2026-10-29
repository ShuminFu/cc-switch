# CCS-199: [i18n] Type the i18n keys and remove the 691 inline defaultValue fallbacks

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:F`, `p3-low`, `area:i18n`

> **Ticket key:** `f-typed-i18n` · **Phase:** F · **Priority:** p3-low · **Effort:** M · **Depends on:** `p0-ci-i18n-parity`, `f-lazy-views-locales`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
Configure i18next CustomTypeOptions from en.json, then remove the defaultValue arguments, adding any missing keys to all locales.

## Acceptance criteria / verification
- [x] `grep -rn defaultValue src | wc -l` is 0 (or an allowlist)
- [x] An unknown key fails `pnpm typecheck`
- [x] The parity test passes

## Dependencies
p0-ci-i18n-parity, f-lazy-views-locales

## Implementation notes

**Summary**
- `src/i18n/i18next.d.ts` declares `CustomTypeOptions` (`defaultNS: "translation"`, `resources.translation: typeof en`), so every `t()` / `i18n.t()` key is checked against `en.json`. `src/i18n/keys.ts` exports `TranslationKey` (`ParseKeys<"translation">`) for key-typed config tables.
- Removed all inline fallbacks from translation calls: 779 `{ defaultValue }` options and 239 positional string fallbacks (`t("key", "文案")`). Every removed key already existed in `en.json`, and the parity test keeps the other locales in sync.
- Typed the static key tables (feature nav, header actions, WebDAV/S3 presets, env badges, workspace files, OMO agent defs, Hermes API modes / base-URL errors, usage-script templates, subscription tiers, skill error codes, data-source labels) and the preset `nameKey` fields as `TranslationKey`. Helpers that take `t` now accept `TFunction`.
- Runtime-dynamic keys: the partner promotion text uses `i18n.exists()` to keep its empty-string fallback, the data-source label uses a key map that falls back to the raw source name, and keys built from preset IDs, provider IDs or backend suggestion codes use an explicit `as TranslationKey` cast, so behaviour is unchanged.
- Typing found two locale bugs, now fixed in all four locales: `usage.appFilter.grokbuild` was missing (the usage hero showed the raw key for Grok Build), and `settings.exportFailedError` had no `{{message}}`, so export errors were never shown to the user.
- Allowlist: the 46 `defaultValue` matches left in `src/` are unrelated to i18n: preset `templateValues[].defaultValue` data, react-hook-form `defaultValues`, and Radix `Tabs`/`Accordion` `defaultValue` props. `tests/i18n/noInlineFallbacks.test.ts` parses `src/` with the TypeScript AST and requires zero `t()` calls with a `defaultValue` option or a positional string fallback. It also has a self-test for the scanner.
- `tests/i18n/typedKeys.test.ts` contains `@ts-expect-error` lines for an unknown key, so `pnpm typecheck` fails if key typing stops working. I checked this by temporarily removing `i18next.d.ts`: tsc then reported `TS2578 Unused '@ts-expect-error' directive` twice.
- Tests that relied on the inline Chinese/English fallbacks now assert keys. Where they check interpolated Chinese text, they load the real zh bundle through `tests/utils/zhTranslations.ts`. Two provider-form golden snapshots now record the error keys in place of the fallback text.
- Removed the stale `src/utils/errorUtils.ts` `no-explicit-any` entry from `eslint-suppressions.json` (that `any` is now `TFunction`).

**Files:** `src/i18n/i18next.d.ts`, `src/i18n/keys.ts`, all four `src/i18n/locales/*.json`, 120+ components, hooks and config modules under `src/` (the fallback removal), `tests/i18n/{noInlineFallbacks,typedKeys}.test.ts`, `tests/utils/zhTranslations.ts`, and the affected component, hook and golden tests.

**Commits:** c080308 (code and tests), plus the follow-up commit that records these notes.

**Verification**
- `pnpm typecheck`: passes (about 28 s).
- `pnpm test:unit`: 119 files, 1232 tests passed.
- `pnpm format:check`: all files formatted.
- `npx vitest run tests/i18n`: parity, usedKeys, missingKeys, lazyLocales, noInlineFallbacks and typedKeys all pass.
- Negative check: with `src/i18n/i18next.d.ts` removed, `tsc --noEmit` fails on `tests/i18n/typedKeys.test.ts` (TS2578 ×2).
- `pnpm lint`: the only errors are 8 `cc-switch/no-app-literal-comparison` hits in `src/components/DeepLinkImportDialog.tsx`, against a suppression budget of 7. They predate this change: the HEAD version of the file without these edits gives the same 8, coming from the concurrent CCS-070 deeplink commits. CCS-199 changes nothing there except removing fallbacks.

