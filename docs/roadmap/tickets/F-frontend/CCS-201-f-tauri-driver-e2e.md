# CCS-201: [e2e] Add a tauri-driver and WebdriverIO smoke on Linux that gates release

**Status:** Partial · **Kind:** chore · **Labels:** `chore`, `phase:F`, `p2-medium`, `area:e2e`

> **Ticket key:** `f-tauri-driver-e2e` · **Phase:** F · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p0-ci-build-smoke`, `p0-ci-release-gated`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
An xvfb job that covers: first run, add a preset, switch and check ~/.claude/settings.json, then takeover on and restore. Run it on the debug build in PR CI and on the AppImage before upload.

## Acceptance criteria / verification
- [ ] The job is green in CI
- [ ] A failing smoke blocks release.yml upload

## Dependencies
p0-ci-build-smoke, p0-ci-release-gated

## Implementation notes

**Status: Partial.** Everything is implemented and passes locally on Linux, both on the debug binary and on a bundled AppImage. "The job is green in CI" can only be confirmed by a GitHub Actions run, which can't happen from this environment. Setting it as a required check is a repo setting, so a maintainer has to do that.

**Summary.**
- New standalone `e2e/` package with its own `pnpm-lock.yaml`, so the root lockfile and the `pnpm audit` gate are unchanged. It uses WebdriverIO 9.32.0 (`@wdio/cli`, `local-runner`, `mocha-framework`, `spec-reporter`, `webdriverio`) and tauri-driver 2.1.0 with WebKitWebDriver (`webkit2gtk-driver`).
  - `e2e/wdio.conf.mjs`:
    - Starts the app with `HOME`, `CC_SWITCH_TEST_HOME` and `XDG_*` all pointing at a fresh temp dir. It drops `ANTHROPIC_*`, `OPENAI_*`, `GEMINI_*`, `GOOGLE_*`, `CLAUDE_*` and `CODEX_*` from the app's environment.
    - Seeds `~/.cc-switch/settings.json` with `{language: "en", enableLocalProxy: true}`. This shows the takeover switch. `firstRunNoticeConfirmed` stays unset, so the app still treats it as a first run.
    - Starts tauri-driver in its own process group in `onPrepare` and kills the whole group, because tauri-driver leaves WebKitWebDriver running. It waits until port 4444 accepts connections before starting.
    - Fails fast with `SevereServiceError` if the binary is missing, port 4444 is already taken, or the driver doesn't start.
    - Mocha `bail`. When a step fails it saves a screenshot and the page text to `e2e/artifacts/`.
    - `CC_SWITCH_E2E_APP` selects the binary or AppImage. The default is `src-tauri/target/debug/cc-switch`. `APPIMAGE_EXTRACT_AND_RUN=1` lets the AppImage run without FUSE.
  - `e2e/specs/smoke.e2e.mjs` runs these steps:
    1. The first-run notice appears, the seeded "Claude Official" card is present and `cc-switch.db` exists. The test then confirms the notice.
    2. Add Provider: dismiss the one-time common-config notice, pick the `DeepSeek` preset, enter an API key and submit. The test waits until the new provider is current in both the live file and the list. The first provider added becomes current.
    3. Switch to Claude Official and check that `~/.claude/settings.json` has no `ANTHROPIC_BASE_URL` or `ANTHROPIC_AUTH_TOKEN`. Switch back to DeepSeek and check the DeepSeek base URL and key.
    4. Turn takeover on: the switch reads `aria-checked=true` and the live `ANTHROPIC_BASE_URL` is `http://127.0.0.1:<port>`, with no real key in the file.
    5. Turn takeover off: the DeepSeek URL and key are restored.
- Test hooks added to `src/`, as attributes only with no behaviour change:
  - `data-testid`: `first-run-notice-confirm`, `add-provider`, `add-provider-submit`, `confirm-dialog-confirm`, `provider-card`, `provider-main-action`, `proxy-takeover-switch`.
  - `data-provider-name` on `ProviderCard`.
  - `data-preset-name` on preset buttons.
- `.github/workflows/ci.yml`: the `tauri-build-smoke` job keeps its id and name, so existing required checks are unaffected. After the debug build it installs `webkit2gtk-driver`, `xvfb` and `tauri-driver@2.1.0`, then runs `xvfb-run -a pnpm --dir e2e test`. On failure it uploads `e2e/artifacts/`. `release.yml` calls `ci.yml`, so the debug smoke also gates tags.
- `.github/workflows/release.yml`, Linux legs (x86_64 and arm64): after `Build Tauri App (Linux)` and before `Prepare Linux Assets` and the upload, it installs the E2E deps and runs the same spec on the built AppImage, then uploads failure artifacts. These steps have no secrets in env and no `continue-on-error`. If the smoke fails, that leg uploads nothing and `publish-release` (`needs: release`) is skipped. This covers acceptance item 2.
- `tests/ci/e2eSmoke.test.ts` is a regression guard. It fails on the pre-change tree (all 3 tests) and passes now. It checks:
  - the step order and commands in both workflows;
  - that the upload step can't run past a failed smoke;
  - that every hook the spec uses exists in `src/`;
  - that the DeepSeek preset name and URL are unchanged.

**Files.** `e2e/{package.json,pnpm-lock.yaml,.gitignore,wdio.conf.mjs,specs/smoke.e2e.mjs}`, `.github/workflows/{ci,release}.yml`, `src/App.tsx`, `src/components/{ConfirmDialog,FirstRunNoticeDialog}.tsx`, `src/components/providers/{AddProviderDialog,ProviderActions,ProviderCard}.tsx`, `src/components/providers/forms/ProviderPresetSelector.tsx`, `src/components/proxy/ProxyToggle.tsx`, `tests/ci/e2eSmoke.test.ts`.

**Commits.** `046534f` (implementation), plus this notes commit.

**Verification** (Ubuntu 24.04, root, Xvfb):
- `pnpm tauri build --debug --no-bundle`, then `xvfb-run -a pnpm --dir e2e test`: 5 passing in about 5s. It passed in 8 of 8 consecutive runs after the final changes, with no leftover driver processes and the temp HOME removed.
- `pnpm tauri build --debug --bundles appimage` (with `createUpdaterArtifacts:false`, since there's no signing key locally), then `CC_SWITCH_E2E_APP=<AppImage> xvfb-run -a pnpm --dir e2e test`: 5 passing, 2 of 2 runs.
- Failure paths all exit 1:
  - a failing step, seen while writing the spec;
  - a missing binary, in 1s;
  - port 4444 already taken, in 2s;
  - `TAURI_DRIVER=/bin/false`, in 2s.
- `pnpm typecheck`: ok. `pnpm lint`: ok. `pnpm format:check`: ok, and `prettier --check` on the `e2e/` files: ok.
- `pnpm test:unit`: 108 files, 1153 tests passed. `vitest run tests/ci`: 92 passed.
- `actionlint` on `ci.yml` and `release.yml`: clean. The `yaml.safe_load` check passes for both.
- No Rust files changed, so no cargo checks were needed.

**Remaining / notes.**
- A GitHub Actions run is needed to confirm acceptance item 1 on `ubuntu-22.04` and on `ubuntu-22.04-arm` (release only). It should then be added as a required check under branch protection.
- A flake showed up while writing the spec. Switching right after adding the first provider sometimes left the list showing the new provider as "In Use" while the live file and DB held the provider just switched to. The likely cause is a late refetch of the current-provider query from the add mutation overwriting the switch result. This was not root-caused. The spec now waits for the add to settle in both the UI and the file before switching. The race itself may deserve its own ticket.

**Review.** The wiring checks out against both acceptance items. The smoke runs after the debug build in `tauri-build-smoke`. On each Linux release leg it runs on the AppImage before `Prepare Linux Assets` and the upload, with no secrets and no `continue-on-error`, and `publish-release` needs the whole matrix. WebView automation is gated only on the `TAURI_WEBVIEW_AUTOMATION` env var that tauri-driver sets, not on debug builds, so the release AppImage can be driven.

Re-running the smoke surfaced a flake that would block releases at random: 4 failures in 31 runs on the debug binary, all at "switched to DeepSeek". The UI and the base URL were correct, but the stored key was `sk-ccswitch-e2e-smoke`. WebKitWebDriver had dropped a keystroke from the typed key, and the add step only checked the base URL, so the bad key surfaced two steps later. This is probably what the earlier "In Use" flake was too, not an app-side race. Follow-up fix:
  - the spec retypes the key until the field holds exactly `API_KEY`;
  - the add step now also checks `ANTHROPIC_AUTH_TOKEN`.

After the fix: 30 of 30 runs pass on the debug binary and 5 of 5 on the AppImage. `pnpm typecheck`, `pnpm format:check` and `pnpm test:unit` (108 files, 1153 tests) all pass. The Status stays Partial until a GitHub Actions run is green.
- `e2e/pnpm-lock.yaml` is not covered by `audit.yml`. It is dev-only test tooling that never ships, and its install scripts stay blocked by pnpm's default.

