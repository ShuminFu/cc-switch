# CCS-199: [i18n] Type the i18n keys and remove the 691 inline defaultValue fallbacks

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:F`, `p3-low`, `area:i18n`

> **Ticket key:** `f-typed-i18n` · **Phase:** F · **Priority:** p3-low · **Effort:** M · **Depends on:** `p0-ci-i18n-parity`, `f-lazy-views-locales`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
Configure i18next CustomTypeOptions from en.json, then remove the defaultValue arguments, adding any missing keys to all locales.

## Acceptance criteria / verification
- [ ] `grep -rn defaultValue src | wc -l` is 0 (or an allowlist)
- [ ] An unknown key fails `pnpm typecheck`
- [ ] The parity test passes

## Dependencies
p0-ci-i18n-parity, f-lazy-views-locales
