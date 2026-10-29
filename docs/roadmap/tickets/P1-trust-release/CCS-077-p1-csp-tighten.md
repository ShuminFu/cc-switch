# CCS-077: [security] Narrow the CSP connect-src and img-src

**Status:** Partial · **Kind:** security · **Labels:** `security`, `phase:P1`, `p3-low`, `area:security`

> **Ticket key:** `p1-csp-tighten` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-csp-too-wide, partially real. The critique notes that "tighten where feasible" had no exit criterion. The webview fetches only https://models.dev and loads https avatars and icons, but the CSP allows `connect-src https: http:` and `img-src http:`.

## Evidence
- src-tauri/tauri.conf.json:29; ModelsDevPickerDialog.tsx:27,164; CopilotAuthSection.tsx:417; ProviderIcon.tsx:85-88

## Proposed change
Set connect-src to `'self' ipc: http://ipc.localhost https://models.dev`, or move the fetch into a command. Remove `http:` from img-src.

## Acceptance criteria / verification
- [ ] `pnpm tauri dev`: the models.dev picker loads, the Copilot avatar and URL icons render, and there are no CSP violations
- [ ] In devtools, `fetch('https://example.com')` is blocked
- [ ] `grep -rn 'fetch(' src` confirms no other direct fetches

## Dependencies
None.

## Implementation notes

**Summary.** `app.security.csp` in `src-tauri/tauri.conf.json` now reads
`connect-src 'self' ipc: http://ipc.localhost https://models.dev` (was `... https: http:`)
and `img-src 'self' data: https:` (`http:` removed). The models.dev picker is the only
direct webview fetch (`grep -rn 'fetch(' src` shows only
`ModelsDevPickerDialog.tsx:164`; every other match is a react-query `refetch()`). No
`XMLHttpRequest` / `WebSocket` / `EventSource` / `sendBeacon` / `convertFileSrc` usage
exists in `src`. Bundled URL icons (`src/icons/extracted`) are local Vite assets
(`'self'`), and the Copilot avatar is served from https (`avatars.githubusercontent.com`),
so both still pass `img-src`. Vite HMR in dev connects to the same origin and is covered
by `'self'`.

A new guard, `tests/lint/csp.test.ts`, parses the CSP and asserts the exact
`connect-src`, that `img-src` has no `http:`, that no directive allows a bare `http:`,
and that only allow-listed files in `src` open direct network connections. Each of those
origins must appear in `connect-src`. With the old config all 4 tests fail; with the new
one all 4 pass.

**Files.** `src-tauri/tauri.conf.json`, `tests/lint/csp.test.ts`.

**Commits.** `1a7c021` fix(security): narrow CSP connect-src and img-src (CCS-077).

**Verification.**
- `npx vitest run tests/lint/csp.test.ts`: 4 passed (4 failed against the HEAD~ config).
- `pnpm typecheck`: clean. `pnpm test:unit`: 133 files / 1330 tests passed.
  `prettier --check` on touched files: clean.
- `cargo fmt --check`: clean. `cargo clippy -- -D warnings`: clean (the config is
  embedded and parsed at compile time). `cargo test --lib`: 2215 passed, 0 failed,
  3 ignored.

**Remaining (needs an interactive GUI session).** Acceptance items 1 and 2 need
`pnpm tauri dev` with devtools open: check that the models.dev picker loads, the Copilot
avatar and URL icons render with no CSP violations, and that
`fetch('https://example.com')` is blocked. They were not run in this headless
environment. Item 3 (the grep for direct fetches) is done and is enforced by the new test.

**Review.** The CSP change, the guard test and the "no other direct fetch" claim all
check out: `src` has a single `fetch(` (`ModelsDevPickerDialog.tsx`, `https://models.dev/api.json`),
and the remaining `http://` matches are SVG `xmlns` attributes, not image loads.
One defect was found. Commit `1a7c021` also deleted
`src/components/providers/forms/ProviderFormFull.tsx`, a deletion another lane had staged.
At that commit `ProviderForm.tsx` still imports `./ProviderFormFull`, so the committed tree
did not build. The review follow-up puts the file back in the tree (index only). It goes
away when the provider-form change that stops importing it is committed. Status stays
Partial until the two `pnpm tauri dev` devtools checks are run in an interactive session.

