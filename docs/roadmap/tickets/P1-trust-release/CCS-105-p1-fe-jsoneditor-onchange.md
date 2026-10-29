# CCS-105: [editor] Stop JsonEditor capturing a stale onChange, which makes the MCP modal overwrite ID and name

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p2-medium`, `area:frontend`

> **Ticket key:** `p1-fe-jsoneditor-onchange` · **Phase:** P1 · **Priority:** p2-medium · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding frontend-jsoneditor-stale-onchange. CodeMirror's update listener keeps the first render's onChange. In Add MCP, every JSON edit that parses as a single key overwrites the typed or preset ID and name. The TOML branch overwrites the ID too.

## Evidence
- src/components/JsonEditor.tsx:145-150, 211 (deps omit onChange); src/components/mcp/McpFormModal.tsx:225-268, 712-724; src/utils/formatters.ts:26-65

## Proposed change
Keep onChangeRef updated on each render and call onChangeRef.current in the listener.

## Acceptance criteria / verification
- [x] Vitest: render with A, rerender with B, a doc change calls B and not A
- [x] McpFormModal test: type id my-id, edit JSON to {"foo":{...}}; the id stays my-id
- [x] `pnpm test:unit` and `pnpm typecheck`

## Dependencies
None.

## Implementation notes
- **Change:** `JsonEditor` keeps the `onChange` prop in `onChangeRef`, refreshed on every render, and the CodeMirror update listener calls `onChangeRef.current(...)`. The editor is still only rebuilt when `darkMode`/`rows`/`height`/`language`/linter change, but edits now always reach the current handler. In `McpFormModal` this means `handleConfigChange` sees the current `formId`/`formName`, so JSON (and TOML) edits no longer overwrite a typed or preset ID and name; auto-extraction still applies when the ID is empty.
- **Files:** `src/components/JsonEditor.tsx`; new tests `tests/components/JsonEditor.test.tsx`, `tests/components/McpFormModal.realEditor.test.tsx` (real CodeMirror editor, not mocked; a referentially stable `t` mock, because a fresh `t` per render rebuilds the editor every render and hides the bug; small jsdom `Range.getClientRects`/`getBoundingClientRect` stubs for CodeMirror measuring).
- **Commits:** `10553b7` (fix + tests); this ticket update is in a follow-up commit.
- **Verification:**
  - Regression proof: with the pre-fix `JsonEditor.tsx`, `npx vitest run tests/components/JsonEditor.test.tsx tests/components/McpFormModal.realEditor.test.tsx` gave 4 failed / 1 passed (rerender A→B test, typed-ID JSON, preset-ID JSON, typed-ID TOML). The control test where the ID is still auto-extracted passed both times. With the fix: 5/5 passed.
  - `pnpm typecheck`: passed.
  - `pnpm test:unit`: 95 files, 663 tests passed.
  - `pnpm format:check`: all files use Prettier style.
  - `pnpm lint`: exit 0.
