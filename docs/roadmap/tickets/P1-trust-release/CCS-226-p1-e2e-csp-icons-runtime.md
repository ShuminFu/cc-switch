# CCS-226: [e2e] Check the tightened CSP and asset-protocol removal at runtime with a headless WebKitGTK spec

**Status:** Done · **Kind:** security · **Labels:** `security`, `phase:P1`, `p2-medium`, `area:e2e`

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
- [x] `pnpm tauri build --debug --no-bundle && pnpm install --dir e2e --ignore-workspace && xvfb-run -a pnpm --dir e2e test` passes.
- [x] Restoring `connect-src ... https: http:` in `src-tauri/tauri.conf.json:29` in a scratch build makes the spec fail.
- [x] CCS-077 and CCS-078 updated to Done with the run recorded as the WebKitGTK substitute for the devtools check.

## Dependencies
None.

## Implementation notes

**Summary.** New WebdriverIO spec `e2e/specs/csp.e2e.mjs`, picked up by the
existing `./specs/**/*.e2e.mjs` glob in `e2e/wdio.conf.mjs` (runs after the smoke,
in the same CI `e2e-smoke` job and the AppImage smoke in `release.yml`). It drives
the real debug binary through tauri-driver / WebKitWebDriver under xvfb:

1. Installs a `securitypolicyviolation` collector in `window.__cspViolations`
   (`blockedURI`, `violatedDirective`, `effectiveDirective`), reads the policy the
   app protocol actually serves (the `Content-Security-Policy` response header of
   `fetch(location.href)`, falling back to a `<meta>` tag) and asserts `connect-src`
   contains `https://models.dev` and no bare `https:`, `http:` or `*`.
2. `fetch('https://example.com/')` rejects **and** a `connect-src` violation for
   example.com is recorded (so CSP, not the network, blocked it).
3. `HEAD https://models.dev/api.json` records no violation (network failure is
   tolerated; offline here it fails with `Load failed` and no violation).
4. An injected `<img>` from `avatars.githubusercontent.com` (the Copilot avatar
   host) records no `img-src` violation; `http://example.com/x.png` errors with an
   `img-src` violation.
5. Adds the `Shengsuanyun` preset (bundled image icon from `iconUrls`) and checks
   every provider-card `<img>` is `complete && naturalWidth > 0`, none uses
   `asset:`/`asset.localhost`, and the inline-SVG brand icons (Claude Official,
   DeepSeek) have a non-zero bounding box. Saves `e2e/artifacts/csp-icons.png`.
6. Visits Settings (General, Routing, Auth, Advanced, Usage Statistics, About; the
   About app icon must load and not use `asset:`), then Skills, Prompts, Session
   Manager and MCP Management, and asserts no violation was recorded meanwhile.

The models.dev picker dialog itself was not opened: step 3 covers its only
request (`ModelsDevPickerDialog.tsx` fetches `https://models.dev/api.json`).

**Files.** `e2e/specs/csp.e2e.mjs` (new); this ticket, CCS-077, CCS-078.

**Commits.** `837cdc0` test(e2e): check the CSP and provider icons at runtime in
WebKitGTK (CCS-226); the ticket updates follow in a docs commit.

**Verification.**
- `pnpm tauri build --debug --no-bundle`: OK. `xvfb-run -a pnpm --dir e2e test`
  (webkit2gtk-4.1, tauri-driver, WebKitWebDriver): 3/3 spec files passed
  (smoke 6, csp 6, session-sync 2), three times. Run output from the csp spec:
  `connect-src: 'self' ipc: http://ipc.localhost https://models.dev; img-src: 'self' data: https:`;
  example.com fetch `TypeError: Load failed` with
  `{"violatedDirective":"connect-src","blockedURI":"https://example.com/"}`; models.dev
  HEAD no violation; https avatar `load`, no violation; http image `error` with an
  `img-src` violation; icons `tauri://localhost/assets/shengsuanyun-*.svg`
  naturalWidth 20, SVGs 20x20; no violations while navigating.
  One of the full runs had the CCS-224 session-sync responsiveness step fail
  transiently; the two re-runs passed (max round trip 69/81 ms against its 250 ms
  limit). It does not involve the CSP spec.
- Negative control: a scratch build with `connect-src ... https://models.dev https: http:`
  fails the spec at "serves the tightened policy" (`Expected value: not "https:"`).
  With the policy assertions commented out in a scratch copy of the spec, it fails at
  "blocks an arbitrary fetch with a connect-src violation" (the fetch still errors
  but no violation is recorded). `tauri.conf.json` was restored and the binary rebuilt
  before the final runs.
- `prettier --check e2e/specs/csp.e2e.mjs`: OK. `pnpm format:check`: OK.
  `pnpm exec vitest run tests/lint/csp.test.ts tests/lint/assetProtocol.test.ts`:
  7 passed. `git grep -n 'convertFileSrc\|asset://' -- src`: no matches.
- No Rust or frontend source changed, so the cargo and unit suites were not re-run.
