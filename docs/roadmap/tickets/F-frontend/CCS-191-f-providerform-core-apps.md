# CCS-191: [provider-form] Extract the Claude, Claude Desktop, Grok, Codex and Gemini form modules

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:F`, `p2-medium`, `area:frontend`

> **Ticket key:** `f-providerform-core-apps` · **Phase:** F · **Priority:** p2-medium · **Effort:** L · **Depends on:** `f-providerform-shell`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
Add features/providers/forms/<app>/{schema,toSettingsConfig,fromProvider,Fields}.tsx for each app, with presets loaded lazily.

## Acceptance criteria / verification
- [ ] Goldens unchanged; unit tests for toSettingsConfig and fromProvider per app
- [ ] `pnpm typecheck` and `pnpm test:unit`

## Dependencies
f-providerform-shell
