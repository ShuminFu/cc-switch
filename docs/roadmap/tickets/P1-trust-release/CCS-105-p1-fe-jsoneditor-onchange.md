# CCS-105: [editor] Stop JsonEditor capturing a stale onChange, which makes the MCP modal overwrite ID and name

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p2-medium`, `area:frontend`

> **Ticket key:** `p1-fe-jsoneditor-onchange` · **Phase:** P1 · **Priority:** p2-medium · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding frontend-jsoneditor-stale-onchange. CodeMirror's update listener keeps the first render's onChange. In Add MCP, every JSON edit that parses as a single key overwrites the typed or preset ID and name. The TOML branch overwrites the ID too.

## Evidence
- src/components/JsonEditor.tsx:145-150, 211 (deps omit onChange); src/components/mcp/McpFormModal.tsx:225-268, 712-724; src/utils/formatters.ts:26-65

## Proposed change
Keep onChangeRef updated on each render and call onChangeRef.current in the listener.

## Acceptance criteria / verification
- [ ] Vitest: render with A, rerender with B, a doc change calls B and not A
- [ ] McpFormModal test: type id my-id, edit JSON to {"foo":{...}}; the id stays my-id
- [ ] `pnpm test:unit` and `pnpm typecheck`

## Dependencies
None.
