# CCS-210: [editor] Stop MarkdownEditor capturing a stale onChange

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:F`, `p3-low`, `area:frontend`

> **Ticket key:** `f-markdowneditor-stale-onchange` · **Phase:** F · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during review of CCS-105 (same defect class as JsonEditor).

## Problem
`src/components/MarkdownEditor.tsx` builds its CodeMirror `EditorView` in an effect whose dependency list is `[darkMode, readOnly, minHeight, maxHeight, placeholderText]` (line ~132). The update listener at line ~71 calls the `onChange` captured when the view was created. When a parent passes a new `onChange` closure (e.g. one that reads fresh state), the editor keeps calling the first one, so parents see stale state.

## Evidence
- `src/components/MarkdownEditor.tsx:71-72` calls the captured `onChange`.
- `src/components/MarkdownEditor.tsx:132` omits `onChange` from the effect dependencies.
- CCS-105 fixed the identical pattern in `src/components/JsonEditor.tsx` with an `onChangeRef`.

## Proposed change
Keep `onChange` in a ref updated on every render and call `onChangeRef.current?.(...)` from the update listener, as in JsonEditor. Do not add `onChange` to the effect dependencies (that would rebuild the editor on each render).

## Acceptance criteria / verification
- [ ] A vitest renders MarkdownEditor with handler A, rerenders with handler B, edits the document via the real EditorView, and asserts B is called and A is not. It fails before the fix.
- [ ] Callers of MarkdownEditor (grep) still behave the same.
- [ ] `pnpm typecheck`, `pnpm test:unit`, `pnpm format:check`, `pnpm lint` pass.

## Dependencies
None.
