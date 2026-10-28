# CCS-193: [i18n] Localize zod and editor validation messages

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:F`, `p3-low`, `area:i18n`

> **Ticket key:** `f-i18n-validation-messages` · **Phase:** F · **Priority:** p3-low · **Effort:** M · **Depends on:** `f-providerform-shell`, `p0-ci-i18n-parity`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding frontend-hardcoded-strings, partially real. Provider Zod messages and parseJsonError are Chinese. validateJsonConfig builds Chinese messages. useCodexConfigState and useManagedAuth use English-only literals. mcp.ts is dead code.

## Evidence
- src/lib/schemas/provider.ts:6-48; src/utils/providerConfigUtils.ts:73-89; useCommonConfigSnippet.ts:261,340; useCodexConfigState.ts:177,181; useManagedAuth.ts:84,120; src/lib/schemas/mcp.ts (unused)

## Proposed change
- Add createProviderSchema(t), and have parseJsonError and validateJsonConfig take t or return codes.
- Add the keys to all 4 locales and delete mcp.ts.

## Acceptance criteria / verification
- [ ] Vitest with en: createProviderSchema(t).safeParse({name:'x',settingsConfig:'{bad'}) contains no CJK (/[一-鿿]/)
- [ ] `grep -rnP '[\x{4e00}-\x{9fff}]' src/lib src/utils src/components/providers/forms/hooks --include=*.ts` matches only comments
- [ ] `pnpm typecheck` and `pnpm test:unit`

## Dependencies
f-providerform-shell, p0-ci-i18n-parity
