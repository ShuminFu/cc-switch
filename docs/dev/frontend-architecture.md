# Frontend architecture and conventions

How the React renderer (`src/`) is organised and the rules new code follows.
Each rule names the module that enforces or implements it.

## Layers and allowed imports

```
types  <-  config  <-  lib/api  <-  lib/query  <-  hooks  <-  components / routes / app
                         (bindings.ts is generated, never edited)
```

| Layer                                     | Holds                                                                                                                | May import                            |
| ----------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | ------------------------------------- |
| `src/types`, `src/types.ts`               | Type declarations only (no runtime data)                                                                             | nothing app-specific                  |
| `src/config`                              | Presets, app registry (`config/apps.ts`), static tables (`config/omo.ts`, `config/providerDefaultConfigs.ts`)        | `types`                               |
| `src/lib/api`                             | One wrapper per backend command; typed IPC via `lib/bindings.ts`                                                     | `types`, `lib/bindings`, `lib/errors` |
| `src/lib/query`                           | TanStack Query hooks, query keys, invalidation                                                                       | `lib/api`, `config`, `lib/*`          |
| `src/lib/*`                               | Framework-light helpers: `events.ts`, `useTauriEvent.ts`, `shortcuts.ts`, `storage.ts`, `errors/*`, `providerCodecs` | `types`, `config`, `utils`            |
| `src/hooks`                               | Feature hooks combining queries and UI concerns (toasts)                                                             | everything above                      |
| `src/components`, `src/routes`, `src/app` | UI                                                                                                                   | everything                            |

Not allowed: `lib/*` importing from `hooks` or `components`; `config` importing
components; toasts in `lib/api`.

## Data fetching

- Every query key comes from `src/lib/query/keys.ts` (lint-enforced). Add new
  keys there, never inline arrays.
- Mutations invalidate through `invalidateProviderCaches` / the helpers next to
  the mutation. Switch and delete are optimistic (`onMutate` + rollback).
- Errors are `unknown` (`Register.defaultError`): always render them with
  `extractErrorMessage(error)` from `src/utils/errorUtils.ts`.

## Errors and toasts

- Backend errors arrive as structured `AppError { code, message, params }`
  (`src/lib/errors/appError.ts`) or plain strings. Classify by `code`
  (`getErrorCode`, `hasErrorCode`), not by substring of the message.
- A failed mutation toasts once, in its `onError`. Callers do not toast again.
- `toast.error` (patched in `src/components/ui/sonner.tsx`) stays 6 s, has a
  close button and a Copy action, and masks secrets (`src/utils/redact.ts`).

## Events

- Backend event names and payloads are the `AppEvents` map in
  `src/lib/events.ts`. `tests/lib/appEvents.test.tsx` fails when Rust emits a
  name that is not in the map or the frontend listens to one that is.
- Subscribe with `useTauriEvent(name, handler)`; do not call `listen` directly.

## Keyboard shortcuts

- Register with `useShortcut` (`src/lib/shortcuts.ts`); the registry owns the
  window listener and the text-field rules. Registered shortcuts are listed in
  the Cmd/Ctrl+K palette (`ProviderCommandPalette`).

## Client-side storage

- Persist through `createStoredValue(key, zodSchema, default)` in
  `src/lib/storage.ts`; the key list lives in that file's header.

## i18n

- User-facing text goes through `t("key", { defaultValue: "English text" })`.
  Default values are English; the four locale files are the source of truth.
- `tests/i18n/keyUsage.test.ts` reports literal keys missing from `en.json`.

## Provider forms

- The form shell picks a per-app adapter (`components/providers/forms/adapters`).
  Per-app knowledge of `settingsConfig` lives in `src/lib/providerCodecs`
  (endpoint, API key, models). `ProviderMeta` fields are documented in
  `src/types.ts`.

## Adding an app or a page

- App: add it to `APP_REGISTRY` (`src/config/apps.ts`), a codec in
  `lib/providerCodecs`, and a form adapter.
- Page: add the view id to `src/app/navigation.tsx` and a route in
  `src/app/routes.tsx`; navigate with `useNavigation().navigate(view)`.
