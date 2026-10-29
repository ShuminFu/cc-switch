# CCS-192: [provider-form] Extract the OpenCode, OpenClaw and Hermes form modules, with component tests

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:F`, `p2-medium`, `area:frontend`

> **Ticket key:** `f-providerform-additive-apps` · **Phase:** F · **Priority:** p2-medium · **Effort:** L · **Depends on:** `f-providerform-shell`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
Add per-app modules, plus component tests for HermesFormFields, OpenClawFormFields and OpenCodeFormFields.

## Acceptance criteria / verification
- [x] Goldens unchanged; ProviderForm.tsx is 300 lines or fewer
- [x] `pnpm typecheck` and `pnpm test:unit`

## Dependencies
f-providerform-shell

## Implementation notes

**Summary.** OpenCode, OpenClaw and Hermes now each have a module under
`src/features/providers/forms/<app>/`, with the same layout CCS-191 gave the
other five apps:

- `schema.ts`: the app's branch of the discriminated union. `providerFormSchemas`
  takes these branches in place of the inline `appVariant(...)` calls.
- `fromProvider.ts`: the editor's initial state (core meta state plus the
  default settingsConfig text). For OpenCode it also returns the saved OMO
  config that seeds the OMO draft, and `isOmoCategory`.
- `toSettingsConfig.ts`: what gets saved. For OpenClaw and Hermes that is the
  trimmed editor text. For OpenCode it is the OMO `{agents, categories?,
  otherFields?}` build, with OMO Slim dropping `categories`. OpenCode also has
  the OMO "other fields" validation and the providerKey to save
  (`omo-xxxxxxxx` / `omo-slim-xxxxxxxx` on create, none on edit). OpenClaw
  also rebases its suggested defaults onto the submitted key.
- `Fields.tsx`: the app's field component plus the settingsConfig JSON editor.
  For OpenCode that is OMO fields plus a read-only merged preview, or the
  regular fields plus the editor.
- `presets.ts`: preset entries with ids `<app>-<index>`. These three tables are
  imported directly rather than through `createPresetLoader`. Their config
  modules are already pulled in statically by `OpenCodeFormFields`
  (`opencodeNpmPackages`), `useOmoModelSource` (`OPENCODE_PRESET_MODEL_VARIANTS`),
  `OpenClawFormFields` (`openclawApiProtocols`) and `HermesFormFields` /
  `useHermesFormState` (API modes, defaults), so lazy loading would not have
  produced a separate chunk. Importing directly also keeps the preset grid
  synchronous, as before.
- The editor (`OpenCodeProviderForm`, `OpenClawProviderForm`,
  `HermesProviderForm`), rendered inside `ProviderFormShell`. None of them
  compares `appId`.

Shared pieces:
- `additiveForm.ts`: `ADDITIVE_APP_META`, existing-key merging, providerKey
  lock state, the providerKey error, managed-auth detection and its login
  error, and the save meta.
- `SettingsConfigJsonField.tsx`: the JSON editor plus its schema error.

Each editor validates in the same order as `ProviderFormFull` and writes the
same payload keys. Keys whose value would be undefined (`providerKey` on OMO
edit, `suggestedDefaults`) are only set when they have a value, as before.

`ProviderFormFull.tsx` is deleted. `PROVIDER_FORM_EDITORS` maps every app to its
own editor. `ProviderForm.tsx` is 44 lines. The 6 `no-app-literal-comparison`
suppressions for `ProviderFormFull` are removed. No backend piece was needed.

**Files.**
- New: `src/features/providers/forms/{additiveForm.ts,SettingsConfigJsonField.tsx}`;
  `src/features/providers/forms/{opencode,openclaw,hermes}/` (`schema`,
  `fromProvider`, `toSettingsConfig`, `Fields`, `presets`, the editor)
- Changed: `src/components/providers/forms/ProviderForm.tsx`,
  `src/features/providers/forms/schema.ts`, `eslint-suppressions.json`
- Removed: `src/components/providers/forms/ProviderFormFull.tsx`
- Tests:
  - New component tests: `tests/components/{HermesFormFields,OpenClawFormFields}.test.tsx`
    (14 and 11 cases).
  - `tests/components/OpenCodeFormFields.test.tsx`: 3 new cases.
  - New unit tests: `tests/features/providerForms/{additive,opencode,openclaw,hermes}.test.ts`
    (25 cases).
  - `tests/features/providerFormShell.test.ts`: checks that the three apps have
    module files, that `ProviderFormFull.tsx` is gone, that every app has its
    own editor, and that the union uses the three new schemas. These checks
    fail on the pre-change tree.

**Commits.** `c036398` (code + tests); this notes edit follows in a separate docs commit.

**Verification.**
- `CI=true npx vitest run tests/components/providerFormGoldens`: 8 files, 467
  tests passed. No snapshot file changed, so the goldens are unchanged, and the
  edit round-trip matches for every OpenCode (OMO included), OpenClaw and
  Hermes preset.
- `CI=true pnpm test:unit`: 139 files, 1384 tests passed.
- `pnpm typecheck`: clean. `pnpm format:check`: clean.
- `npx eslint src/features src/components/providers/forms tests/features` plus
  the three FormFields tests: clean. A full `pnpm lint` reports 20 errors, all
  in files this ticket does not touch (`src/App.tsx`,
  `src/components/DeepLinkImportDialog.tsx`,
  `src/components/providers/ProviderActions.tsx`). Those files have more
  app-literal comparisons than their entries in `eslint-suppressions.json`
  allow. This was already the case at HEAD before this change.
