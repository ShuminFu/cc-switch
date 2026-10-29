# CCS-191: [provider-form] Extract the Claude, Claude Desktop, Grok, Codex and Gemini form modules

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:F`, `p2-medium`, `area:frontend`

> **Ticket key:** `f-providerform-core-apps` · **Phase:** F · **Priority:** p2-medium · **Effort:** L · **Depends on:** `f-providerform-shell`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
Add features/providers/forms/<app>/{schema,toSettingsConfig,fromProvider,Fields}.tsx for each app, with presets loaded lazily.

## Acceptance criteria / verification
- [ ] Goldens unchanged; unit tests for toSettingsConfig and fromProvider per app
- [ ] `pnpm typecheck` and `pnpm test:unit`

## Dependencies
f-providerform-shell

## Implementation notes

**Summary.** Claude, Claude Desktop, Codex, Gemini and Grok Build now each
have a module under `src/features/providers/forms/<app>/`:

- `schema.ts`: the app's branch of the discriminated union.
  `providerFormSchemas` in `schema.ts` takes these branches instead of
  building them inline. `appVariant` and the base schema moved to
  `baseSchema.ts` so the per-app modules and `schema.ts` don't import each
  other in a cycle.
- `fromProvider.ts`: pure functions that turn a saved provider (or none, on
  create) into the editor's initial state. That covers the default
  settingsConfig text, the API format and API key field inference, Codex
  wire_api inference, Grok Build config.toml parsing, Claude Desktop route
  rows and the shared `coreFormFromProvider` part.
- `toSettingsConfig.ts`: pure functions that build what gets saved. That
  covers the settingsConfig string (the Codex `{auth, config, modelCatalog}`
  build, the Gemini `{env, config}` build, the Grok Build TOML write-back,
  and the Claude Desktop env/route map, including the route validation
  errors) and the app-owned meta fields.
- `Fields.tsx`: the app's fields. They read and write settingsConfig through
  `useFormContext`.
- `presets.ts`: a lazily imported preset table (`createPresetLoader` and
  `useLazyPresets` in `lazyPresets.ts`), plus the preset-entry id scheme.
  Grok Build reuses the Codex loader.
- The editor component (`ClaudeProviderForm`, `CodexProviderForm`,
  `GeminiProviderForm`, `ClaudeDesktopProviderForm`,
  `GrokBuildProviderForm`). It wires hooks and state to those functions
  inside `ProviderFormShell`.

The shared pieces are in `coreForm.ts` and `CommonConfigNotice.tsx`.
`coreForm.ts` holds pricing, draft custom endpoints, the payload base, and
`coreProviderMeta`, which writes the meta keys in the same order and with the
same "undefined clears it" values as before. `CommonConfigNotice.tsx` is the
one-time common-config dialog.

`PROVIDER_FORM_EDITORS` maps the five apps to their new editors.
`ProviderFormFull.tsx` now serves only OpenCode, OpenClaw and Hermes, which
are left for CCS-192. It is 847 lines, down from 2100, and its
claude/codex/gemini branches are gone. The old component paths
`ClaudeDesktopProviderForm.tsx` and `GrokBuildProviderForm.tsx` are re-export
shims.

Presets are loaded lazily. `AddProviderDialog` awaits the loaders when it
looks up preset endpoints on submit. `ClaudeFormFields` reads the cached
Claude table for `modelsUrl`. `useProviderCategory` takes the app's preset
list as a parameter instead of importing all six. `CLAUDE_DESKTOP_ROLE_ROUTE_IDS`
moved to `src/config/claudeDesktopRoles.ts` and is re-exported from the
presets file. After these changes, no static importer remains: a production
`vite build` emits `claudeProviderPresets`, `codexProviderPresets`,
`geminiProviderPresets` and `claudeDesktopProviderPresets` as separate chunks.
`useGeminiConfigState` reuses the pure `.env` helpers from the gemini module.
The eslint suppressions that no longer match anything were pruned:
`no-explicit-any`, `no-empty` and one app-literal entry for `ProviderFormFull`,
and all of `useProviderCategory`'s.

No backend piece was needed.

**Files.**
- New: `src/features/providers/forms/{baseSchema,coreForm,lazyPresets}.ts`,
  `CommonConfigNotice.tsx`; `src/features/providers/forms/{claude,claude-desktop,codex,gemini,grokbuild}/`
  (`schema`, `fromProvider`, `toSettingsConfig`, `Fields`, `presets`, the editor;
  `claude-desktop/routes.ts`); `src/config/claudeDesktopRoles.ts`
- Changed: `src/features/providers/forms/schema.ts`,
  `src/components/providers/forms/{ProviderForm,ProviderFormFull,ClaudeDesktopProviderForm,GrokBuildProviderForm,ClaudeFormFields}.tsx`,
  `src/components/providers/forms/hooks/{useProviderCategory,useGeminiConfigState}.ts`,
  `src/components/providers/AddProviderDialog.tsx`,
  `src/config/claudeDesktopProviderPresets.ts`, `eslint-suppressions.json`
- Tests: `tests/features/providerForms/{claude,codex,gemini,claudeDesktop,grokbuild,lazyPresets}.test.ts`
  (new), `tests/features/providerFormShell.test.ts`,
  `tests/components/GrokBuildProviderForm.test.tsx`. The preset grid now
  appears once the lazy import resolves, so that test waits for it with
  `findByRole`.

**Commits.** `9a3b4c2` (code + tests); this notes edit follows in a separate docs commit.

**Verification.**
- `CI=true npx vitest run tests/components/providerFormGoldens`: 8 files, 467
  tests passed. No snapshot files changed, so the goldens are unchanged. The
  edit round-trip still matches for every preset.
- New unit tests, 47 cases, cover `toSettingsConfig` and `fromProvider` for
  each of the five apps, plus the lazy loader. The loader tests check caching,
  retry after a failed import, the hook's async-then-sync behaviour, and a
  guard that the editors, `AddProviderDialog`, `ClaudeFormFields` and
  `useProviderCategory` do not import the preset modules statically. The
  guard fails on the pre-change sources. `providerFormShell.test.ts` adds 2
  cases: each app has its module files, and the union uses each module's
  schema. All of these fail before the change because the modules did not
  exist.
- `CI=true pnpm test:unit`: 132 files, 1326 tests passed.
- `pnpm typecheck`: clean. `pnpm format:check`: clean.
- `npx eslint src/components/providers/forms src/features src/config tests/features src/components/providers/AddProviderDialog.tsx`:
  clean.
- `npx vite build`: the four preset chunks listed above are emitted
  separately.
