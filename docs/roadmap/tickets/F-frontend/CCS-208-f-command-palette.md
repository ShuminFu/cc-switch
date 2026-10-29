# CCS-208: [shell] Add a Cmd/Ctrl-K command palette to search and switch providers across apps

**Status:** Done · **Kind:** feature · **Labels:** `feature`, `phase:F`, `p3-low`, `area:frontend`

> **Ticket key:** `f-command-palette` · **Phase:** F · **Priority:** p3-low · **Effort:** S · **Depends on:** `f-query-keys`, `f-switching-state`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
This combines the plan backlog item with frontend idea 6. cmdk is already a dependency, and shortcuts are scattered across 8 apps.

## Proposed change
A palette listing providers for all visible apps (from the registry and query keys) plus common actions (toggle proxy, open settings).

## Acceptance criteria / verification
- [x] RTL: Ctrl-K opens the palette; typing filters; Enter switches the provider
- [x] `pnpm test:unit`

## Dependencies
f-query-keys, f-switching-state

## Implementation notes

**Summary.** Added a Cmd/Ctrl-K command palette (cmdk inside a Radix dialog) to the app shell.

- Lists the providers of every visible app. Apps come from `useAppRegistry()` (`src/entities/app/registry.ts`), which is still static. When the `get_app_registry` IPC lands, the palette picks up backend-provided apps and capabilities through that hook without any change of its own. Provider data is loaded with `useQueries` on `providerKeys.list(appId)`, the same key and fetcher as `useProvidersQuery`. To share that fetcher, it was extracted as `fetchProvidersForApp` in `src/lib/query/queries.ts`. Provider data is fetched only while the palette is open.
- Filtering matches only the app label and provider name (cmdk `keywords` plus a custom `filter`), so a search never hits characters inside provider IDs or UUIDs.
- Pressing Enter on a provider calls `switchProvider` from `useProviderActions`, so the proxy-required warnings, the official-provider takeover block, toasts and the CCS-207 switching state all still apply. If the provider belongs to another app, the palette switches `activeApp` first and runs the switch on the next render, once the actions are bound to the target app. For additive apps (OpenCode, OpenClaw, Hermes) and for the current provider, Enter only navigates to that app's provider list.
- Actions: add a provider to the active app, enable or disable proxy takeover for the active app, and open settings. The takeover action is shown under the same conditions as the header `ProxyToggle`: `enableLocalProxy` is on and the active app is not Claude Desktop.
- Shortcut: Cmd/Ctrl-K, with no Shift or Alt, toggles the palette. It is ignored when the event is already `defaultPrevented` (for example by an editor) and does not open while another dialog is open.
- i18n: added a `commandPalette.*` section in en, zh, zh-TW and ja.

**Files.**
- `src/components/app-shell/CommandPalette.tsx` (new: `CommandPalette` and `useCommandPaletteShortcut`)
- `src/App.tsx`
- `src/lib/query/queries.ts`
- `src/i18n/locales/{en,zh,zh-TW,ja}.json`
- `tests/components/CommandPalette.test.tsx` (new)
- `tests/integration/App.test.tsx`
- `tests/setupGlobals.ts` (adds a `scrollIntoView` polyfill for cmdk in jsdom)

**Commits.** 3c59e50 (feature), plus the follow-up commit that records these notes.

**Verification.**
- `pnpm vitest run tests/integration/App.test.tsx`: 8/8 passed. The new test presses Ctrl-K on the real `App` with MSW, checks that providers from Claude, Codex and Gemini are listed, types "codex second" and checks the list is filtered, then presses Enter. It asserts that `providersApi.switch("codex-2", "codex")` was called, the active app is now Codex and the palette has closed. It also checks that Ctrl-K is ignored while the first-run notice dialog is open. The test cannot pass before this change because there was no palette.
- `pnpm vitest run tests/components/CommandPalette.test.tsx`: 7/7 passed. Covers: toggling with Cmd/Ctrl-K, no Shift combination, no opening over another dialog, visible-apps filtering, no match on provider IDs, Enter switching a provider, navigate-only for the current provider and additive apps, and the actions (settings, takeover shown or hidden).
- `pnpm test:unit`: 123 files, 1255 tests, all passed.
- `pnpm typecheck`: clean.
- `pnpm exec prettier --check` on every touched file: clean. `pnpm format:check` reports only files belonging to the concurrent deeplink work, which are not part of this ticket.
- No Rust files were touched.
