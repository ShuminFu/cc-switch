# CCS-190: [provider-form] Turn ProviderForm into a shell with a base schema, a discriminated union and an inline error summary

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:F`, `p2-medium`, `area:frontend`

> **Ticket key:** `f-providerform-shell` · **Phase:** F · **Priority:** p2-medium · **Effort:** L · **Depends on:** `f-settingsconfig-goldens`, `p2-fe-app-registry`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Evidence
- src/components/providers/forms/ProviderForm.tsx (2578 lines)

## Proposed change
- Add features/providers/forms/ with a base schema plus a per-app discriminated union, and Fields components using useFormContext.
- Add a shared BasePreset.
- Add an inline error summary that blocks Save across all editors (frontend idea 1).

## Acceptance criteria / verification
- [x] The shell is 300 lines or fewer; the settingsConfig goldens are unchanged
- [x] `pnpm typecheck` and `pnpm test:unit`

## Dependencies
f-settingsconfig-goldens, p2-fe-app-registry

## Implementation notes

**Summary.** New `src/features/providers/forms/` holds the shared form shell:

- `schema.ts`: `baseProviderFormSchema` (the existing provider fields) and
  `providerFormSchema`, a `z.discriminatedUnion("appId", ...)` with one
  variant per app (`providerFormSchemas`, typed to cover every `AppId`).
  `providerFormResolver(appId)` validates through the union. It adds `appId`
  before validation and removes it afterwards, so the submitted payload does
  not change. The variants only hold the base fields for now; CCS-191/192 add
  app-specific fields to them.
- `basePreset.ts`: `BasePreset`, the fields all presets share (name, nameKey,
  websiteUrl, apiKeyUrl, partner flags, category, endpointCandidates, icon).
  All seven preset interfaces in `src/config/*ProviderPresets.ts` now extend
  it. Helpers: `presetBaseFormValues` (the form reset that each preset branch
  used to repeat) and `activePresetFrom`.
- `useSaveGate.ts` + `ProviderFormErrorSummary.tsx`: the inline error summary
  (frontend idea 1). Calling `gate.block(message)` records a blocking error
  and also shows the toast it used to. The summary (`role="alert"`,
  `#provider-form-error-summary`) sits right above Save. It lists those errors
  together with the schema field errors (for example invalid settingsConfig
  JSON, which used to show only under the editor) and scrolls itself into
  view. Save does not happen while any error remains. Each submit clears the
  list and checks again. The gate also owns the "save anyway" soft-issue
  confirmation, which used to be three pieces of pending state in the form.
- `ProviderFormShell.tsx` (114 lines): it provides the FormProvider, the
  `#provider-form` element, the summary, the Cancel/Save buttons and the
  soft-issue dialog. `useProviderForm(appId, defaults)` wires the union
  resolver. All editors render inside the shell: the multi-app body, Claude
  Desktop and Grok Build. That makes the summary apply to every editor.
- Fields that use `useFormContext`: `BasicFormFields` (it no longer takes a
  `form` prop), `SettingsConfigErrorField` and the summary.
  `AdditiveProviderKeyField` replaces the three copy-pasted provider-key
  inputs for opencode, openclaw and hermes, and their three copies of the
  submit checks. The checks run in the same order with the same messages.

`ProviderForm.tsx` is now a 39-line shell. `PROVIDER_FORM_EDITORS` is a
`Record<AppId, ...>`, so registering a new app in the registry fails the type
check until it has an editor. It re-exports the types and save helpers that
existing callers and tests import. The multi-app editor body moved unchanged
(apart from the extractions above) to `ProviderFormFull.tsx`, 2100 lines.
CCS-191/192 split it into `features/providers/forms/<app>/` modules. The
pure save helpers moved to `saveHelpers.ts`. The app-literal ESLint
suppression moved with the body, from 9 entries to 7, because the dispatcher
no longer compares app literals.

No backend piece was needed: the editor map is keyed by the static frontend
app registry (CCS-119).

**Files.**
- `src/features/providers/forms/{schema,basePreset,saveHelpers,types,useSaveGate}.ts`, `{ProviderFormShell,ProviderFormErrorSummary,SettingsConfigErrorField,AdditiveProviderKeyField}.tsx`, `index.ts`
- `src/components/providers/forms/ProviderForm.tsx`, `ProviderFormFull.tsx` (new), `ClaudeDesktopProviderForm.tsx`, `GrokBuildProviderForm.tsx`, `BasicFormFields.tsx`
- `src/config/{claude,claudeDesktop,codex,gemini,opencode,openclaw,hermes}ProviderPresets.ts`
- `src/i18n/locales/{en,ja,zh,zh-TW}.json` (`providerForm.errorSummary.title`)
- `eslint-suppressions.json`
- `tests/components/ProviderFormErrorSummary.test.tsx`, `tests/features/providerFormShell.test.ts`

**Commits.** `1662908` (code + tests); this notes edit follows in a separate docs commit.

**Verification.**
- `CI=true npx vitest run tests/components`: 48 files, 699 tests passed. That
  includes all 8 settingsConfig golden files (467 cases) with no snapshot
  changes, plus the parse-guard, Grok Build and Claude Desktop form tests.
- New regression tests: `ProviderFormErrorSummary.test.tsx` (5 cases). They
  cover a blocked Codex save listed inline, then cleared once the next save
  passes; an OpenCode save with no provider key; an invalid settingsConfig
  JSON schema error; a Claude Desktop save with no name; and Grok Build
  required fields. With the summary disabled (it renders nothing), all 5 fail.
  `providerFormShell.test.ts` (6 cases) checks that the union has one variant
  per registered app, that the resolver removes `appId` and reports JSON
  errors, that every preset type extends `BasePreset` (type-level), the
  preset helpers, that every app has an editor, and that `ProviderForm.tsx`
  and the shell are each 300 lines or fewer.
- `CI=true pnpm test:unit`: 125 files, 1276 tests passed.
- `pnpm typecheck`: clean. `pnpm format:check` (prettier over src and tests): clean.
- `npx eslint src/components/providers/forms src/features src/config tests/features tests/components/ProviderFormErrorSummary.test.tsx`:
  clean. A full `pnpm lint` still reports 20 errors that existed before this
  change, all in files it does not touch (`App.tsx`, `DeepLinkImportDialog.tsx`,
  `ProviderActions.tsx`).
