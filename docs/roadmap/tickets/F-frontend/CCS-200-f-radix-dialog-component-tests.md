# CCS-200: [ui] Move custom overlays to Radix Dialog and add component tests for the large panels

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:F`, `p3-low`, `area:frontend`

> **Ticket key:** `f-radix-dialog-component-tests` · **Phase:** F · **Priority:** p3-low · **Effort:** M · **Depends on:** `f-providerform-core-apps`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
- Replace the custom `fixed inset-0` overlays with Radix Dialog (focus trap, Esc).
- Add component tests for CodexFormFields, ProxyPanel and UsageScriptModal.

## Acceptance criteria / verification
- [x] `grep -rn 'fixed inset-0' src/components` matches only Radix internals
- [x] The new tests pass under `pnpm test:unit`

## Dependencies
f-providerform-core-apps

## Implementation notes

**Summary**
- `src/components/common/FullScreenPanel.tsx`: the panel is now a Radix Dialog
  (`Root` + `Portal forceMount` + `Content forceMount asChild` wrapping the
  existing framer-motion container, header text is `Dialog.Title`). This gives
  every full-screen panel (UsageScriptModal, PricingEditModal, McpFormModal,
  provider add/edit, prompt/workspace panels, ...) a focus trap, `role="dialog"`
  + `aria-modal`, and Radix's layered Esc handling (an open Select/Dropdown or
  nested Dialog consumes Esc first). The hand-rolled `createPortal` + window
  `keydown` listener is gone; Esc from a text input/textarea/contenteditable is
  still ignored via `onEscapeKeyDown`. The body `overflow: hidden` effect is
  kept because `useViewShortcuts` relies on it.
- `src/components/skills/UnifiedSkillsPanel.tsx`: the "import skills" overlay
  (`fixed inset-0 bg-black/50` div) now uses the shared `Dialog`/`DialogContent`
  (`zIndex="alert"`, same as the restore dialog) with header/description/footer;
  Esc/close is ignored while an import is in flight.
- `grep -rn 'fixed inset-0' src/components` now matches only
  `ui/dialog.tsx` (the Radix wrapper) and `common/FullScreenPanel.tsx` (Radix
  `Dialog.Content`). `tests/lint/customOverlays.test.ts` enforces this: any
  component using `fixed inset-0` must import `@radix-ui/react-dialog`.
- Behaviour note: Radix calls `preventDefault()` on an Esc keydown it handles
  (capture phase on `document`), so inside a full-screen panel an editor's own
  Esc binding (e.g. CodeMirror completion close) now sees a prevented event,
  matching what already happened inside every other Radix dialog.

**Tests added**
- `tests/components/CodexFormFields.test.tsx` (10): official vs third-party
  visibility, default model, advanced section auto/ manual expand, Anthropic-only
  controls + digit stripping, reasoning effort implies thinking, catalog rows
  reported without `rowId`, "add default model to mapping", model fetch guard
  and request identity, endpoint speed-test modal.
- `tests/components/ProxyPanel.test.tsx` (14): stopped state (config seeding,
  LAN warning, address/port validation, localhost normalisation, IPv6, service
  switch) and running state (address/IPv6 bracketing, stats, takeover
  success/failure toasts, failover queue, logging toggle).
- `tests/components/UsageScriptModal.test.tsx` (14), rendered with the real
  FullScreenPanel: dialog role/name + focus inside, Esc closes, Esc in input
  does not, back button, closed state, first-enable confirm (nested Esc closes
  only the confirm dialog), confirmation persisted, save validation, template
  save, native templates, test-script success and bare-string rejection,
  official-provider template filtering.
- `tests/components/UnifiedSkillsPanel.test.tsx` (+1): import dialog is an
  accessible Radix dialog and closes on Esc.
- Regressions: the dialog-role / focus tests and the lint guard fail on the
  previous implementation (no `role="dialog"`, no Radix import in either file).

**Commits:** a5d44ed (code + tests), plus the follow-up commit recording these notes.

**Verification**
- `pnpm typecheck` — clean.
- `pnpm format:check` — "All matched files use Prettier code style!"
- `npx eslint` on the touched files — clean.
- `pnpm test:unit` — 145 files / 1433 tests passed.
- `grep -rn 'fixed inset-0' src/components` — only `ui/dialog.tsx` and the Radix
  content in `common/FullScreenPanel.tsx`.
- No files under `src-tauri/` were touched.
