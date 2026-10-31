# Release checklist: automated journeys

Which release-checklist item each automated end-to-end check covers (TEST-17, PRODUCT-45), and
what is still checked by hand before tagging. `RELEASING.md` links here.

## Renderer journeys (`pnpm test:e2e`, every PR)

Playwright drives the real renderer (`vite` dev server) in Chromium. Tauri IPC is mocked by
`e2e/ipc-mock.ts`, which answers every `invoke` with the same MSW handlers the vitest suite uses
(`tests/msw/handlers.ts`). A journey can change what a command returns before the app boots with
`withPage(app, name, run, { setup })` (see `e2e/harness.mjs`). On failure a screenshot and a
Playwright trace are written to `e2e/artifacts/` and uploaded by CI.

| Journey | Release-checklist item it replaces |
| --- | --- |
| J1 boot | App starts, provider list renders, no unmocked startup command |
| J2 switch | Switching a Claude provider calls `switch_provider`, moves "In Use", refreshes the tray |
| J3 apps | The app switcher loads each app's own providers |
| J4 switch failure | A backend error shows the localized error toast and changes nothing |
| J5 settings | The settings page opens with its tabs |
| J6 add provider | The add-provider dialog opens with the preset catalog |
| J7 switch per app | A Codex switch goes through `app: "codex"` and leaves Claude's current provider alone |
| J8 settings save | Toggling a General setting saves it (`save_settings`) and the page shows the new value |
| J9 export / import | SQL backup export writes to the chosen path; import asks for confirmation first and imports only after it |
| J10 failover toggle | With takeover active, the header failover switch calls `set_auto_failover_enabled` for that app |
| J11 deep link | A provider deep link shows a preview, imports only on confirm, and never switches without the opt-in |

Run locally (Playwright is not a project dependency):

```bash
npm i -g playwright@1 && npx playwright install chromium
NODE_PATH="$(npm root -g)" pnpm test:e2e
```

## Backend scenario (`cargo test`, every PR, all three OSes)

`src-tauri/tests/scenario_takeover_failover.rs` runs switch -> takeover -> failing upstream ->
failover -> takeover off on a temporary HOME with two mock upstreams, and checks
`~/.claude/settings.json` byte for byte: no proxy residue after takeover ends, and the restored
file equals what a plain switch to the current provider writes (the "takeover residue" and
"placeholder backup / restore loop" bug classes of v3.16.2 and v3.16.3). The proxy request path
itself is covered by `src-tauri/tests/proxy_e2e.rs`.

## Real binary smoke (nightly, Linux)

The `tauri-smoke` job in `.github/workflows/nightly.yml` builds a debug app, launches it under
`tauri-driver` + `xvfb` with HOME in a temporary directory seeded with a Claude Code config, and
checks that the window renders, `~/.cc-switch/cc-switch.db` is created and the seeded
`~/.claude/settings.json` is left byte-identical (`scripts/tauri-smoke.mjs`). It is
`continue-on-error` until it has been green for a week.

## Still manual before a release

- Tray menu: switch a provider from the tray on each OS.
- System proxy / takeover against a real CLI session (Claude Code, Codex) on each OS.
- Updater: an installed older build offers and installs the update (RELEASING.md, step 5).
- Window chrome on Linux (Wayland and X11) and the AppImage.
