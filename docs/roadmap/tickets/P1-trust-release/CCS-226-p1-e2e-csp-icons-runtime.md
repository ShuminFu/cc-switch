# CCS-226: [e2e] Check the tightened CSP and asset-protocol removal at runtime with a headless WebKitGTK spec

**Status:** Open · **Kind:** security · **Labels:** `security`, `phase:P1`, `p2-medium`, `area:e2e`

> **Ticket key:** `p1-e2e-csp-icons-runtime` · **Phase:** P1 · **Priority:** p2-medium · **Effort:** M
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-077, CCS-078, CCS-002). Source tickets: [CCS-077](../P1-trust-release/CCS-077-p1-csp-tighten.md), [CCS-078](../P1-trust-release/CCS-078-p1-asset-protocol-remove.md), [CCS-002](../P1-trust-release/CCS-002-epic-p1.md).

## Problem
CCS-077 (items 1-2) and CCS-078 (AC2 "icons still render") are Partial only because they say "check in `pnpm tauri dev` with devtools". The CCS-201 harness drives the real binary headlessly (`e2e/wdio.conf.mjs`, `e2e/specs/*.e2e.mjs`, tauri-driver, WebKitWebDriver, webkit2gtk-4.1 2.52.6 under xvfb-run), so both can be automated against the build that actually ships. The static halves are met: `tests/lint/csp.test.ts`, `tests/lint/assetProtocol.test.ts`, and `git grep convertFileSrc\|asset:// -- src` is empty.

## Proposed change
Add `e2e/specs/csp.e2e.mjs` (and optionally run the collector during `smoke.e2e.mjs`):
- `before()`: register a `securitypolicyviolation` listener collecting `{blockedURI, violatedDirective}` in `window.__cspViolations`; read the effective policy and assert `connect-src` has no bare `https:`.
- Arbitrary fetch: `fetch('https://example.com')` via `executeAsync` is blocked AND a `connect-src` violation on example.com is recorded (proves CSP, not network, blocked it).
- models.dev: a HEAD to `https://models.dev/api.json` records no violation (network failure is fine). Optionally open the picker (`PricingEditModal.tsx:227` → `ModelsDevPickerDialog.tsx:164`).
- Images: an injected `<img src="https://avatars.githubusercontent.com/u/9919?v=4">` records no img-src violation; an injected `http://example.com/x.png` does.
- Icons: after first run, every provider-card `img` has `complete && naturalWidth > 0` and no `src` starts with `asset:`; inline-SVG icons have a non-zero bounding box; optionally Settings > About appIcon loads. Save a screenshot to `e2e/artifacts`.
- `__cspViolations` is empty after navigating the main views.
- Then tick CCS-077 items 1-3 (item 3 is enforced by `tests/lint/csp.test.ts`; note the ProviderFormFull.tsx review defect is resolved) and CCS-078 AC2, and set both to Done.

## Acceptance criteria / verification
- [ ] `pnpm tauri build --debug --no-bundle && pnpm install --dir e2e --ignore-workspace && xvfb-run -a pnpm --dir e2e test` passes.
- [ ] Restoring `connect-src ... https: http:` in `src-tauri/tauri.conf.json:29` in a scratch build makes the spec fail.
- [ ] CCS-077 and CCS-078 updated to Done with the run recorded as the WebKitGTK substitute for the devtools check.

## Dependencies
None.
