# Dioxus Frontend Migration Plan

Status: in progress on branch `claude/modest-turing-ywec52`. Phase 0 done
(see `dioxus-dev-setup.md`); Phase 1 in progress.

Deviation from §4: the presets and config utilities move into shared Rust
crates (`crates/cc-switch-presets`, `crates/cc-switch-config`) that both the
backend and the wasm32 frontend can link, instead of behind new Tauri
commands. The Dioxus UI gets synchronous access like the React app had, and
the backend can adopt the same crates later. The React app keeps its TS
copies until Phase 6; `pnpm presets:dump` regenerates the JSON from the TS
catalogs and CI fails when they drift.

Deviation from §3.1: `src-tauri` stays a standalone package and the new
crates form their own workspace under `crates/` (see the setup doc for why).

This document plans the rewrite of the cc-switch frontend from React/TypeScript
to Rust with [Dioxus](https://github.com/DioxusLabs/dioxus), while keeping the
existing Tauri 2 shell and the Rust backend in `src-tauri/`. It is based on a
full inventory of the current frontend taken on 2026-09-16 (v3.17.0).

## 1. Goals and non-goals

Goals

- One language across the app. The backend is already ~155k lines of Rust; the
  frontend (~96k lines of TS/TSX/JSON) is the last non-Rust part.
- Delete the hand-mirrored type layer (109 TypeScript interfaces mirroring Rust
  structs) and the parse/merge logic that today exists in both languages.
- Keep shipping the React UI until the Dioxus UI reaches feature parity. No
  big-bang cutover.
- Keep Tailwind, the design tokens in `src/index.css`, and the four locale
  files, so the app looks and reads the same.

Non-goals

- Replacing Tauri with the Dioxus desktop renderer. The tray, updater,
  deep-link, single-instance and window-state plugins, the 285 commands and the
  packaging pipeline all live in Tauri. Dioxus will be the *web* renderer
  (WASM) running inside the Tauri webview.
- Redesigning the UI. This is a port, not a redesign.
- Rewriting the backend. Only logic that is duplicated in the frontend moves.

## 2. Inventory summary (what has to be ported)

| Area | Files | Lines | Notes |
|---|---:|---:|---|
| `src/components/providers` | 64 | 22,107 | Provider list, cards, per-vendor forms (55 files in `forms/`), 22 form hooks. A quarter of the whole port. |
| `src/i18n` (4 JSON locales) | 5 | 11,964 | 2,583 keys x 4 (zh-TW is 28 keys behind). Reused verbatim. |
| `src/config` (preset catalogs) | 15 | 11,227 | Pure data plus small shaping functions. Moves to the backend, not ported. |
| `src/components/settings` | 21 | 6,650 | 6 tabs plus nested accordion sub-panels. |
| `src/lib` (api, query, schemas) | 53 | 6,016 | 26 invoke modules, 11 react-query modules, 4 zod schema files. |
| `src/components` root | 19 | 5,106 | App shell pieces, editors, pickers, footers. |
| `src/components/usage` | 14 | 4,105 | Usage dashboard incl. one recharts chart. |
| `src/hooks` | 26 | 3,728 | |
| `src/utils` | 17 | 2,725 | ~1.8k lines of TOML/JSON config surgery moves to the backend. |
| sessions, proxy, skills, mcp, universal, openclaw, workspace, prompts, profiles, misc | 51 | ~12,700 | |
| `src/components/ui` (shadcn primitives) | 23 | 1,544 | Replaced by `dioxus-primitives`. |
| `src/App.tsx` + `main.tsx` | 2 | 1,791 | No router; two persisted enums (`activeApp` x `currentView`) and a `switch`. |
| `tests/` (vitest + MSW fake backend) | 68 | 13,558 | Does not transfer; rewritten per §7. |

Roughly 24k of the 96k lines are data and ~13k more are movable to Rust or
deleted outright. The real UI surface to port is about 60k lines of TSX, which
should land at roughly 45k to 55k lines of `rsx!` Rust.

Frontend-backend contract: 273 distinct Tauri commands are invoked (of 285
registered; the 12 unused ones can be dropped) and 12 live events are
listened to. Only three Tauri JS plugins are used from the frontend
(`updater`, `process`, `dialog`), plus `window`, `app` and `path` from the core
API.

Third-party libraries that need a replacement decision are listed in §5.

## 3. Target architecture

```
cc-switch/
  Cargo.toml                 # NEW root workspace
  src-tauri/                 # unchanged role: Tauri shell + backend (cc_switch_lib)
  crates/
    cc-switch-contract/      # NEW: serde types + command/event names shared by both sides
    cc-switch-ui/            # NEW: Dioxus 0.7 web app (wasm32), built with `dx`
  scripts/icon-tools/        # existing Rust tool, becomes a workspace member
  src/                       # React app, kept until parity, then deleted
```

### 3.1 Root Cargo workspace (Phase 0)

Members: `src-tauri`, `crates/*`, `scripts/icon-tools`. Consequences that
must be handled in the same PR:

- Hoist `[profile.release]` from `src-tauri/Cargo.toml` to the root
  (`codegen-units = 1`, `lto = "thin"`, `opt-level = "s"`, `panic = "unwind"`,
  `strip = "symbols"`). Cargo ignores profiles in non-root members and the
  `panic = "unwind"` setting is required by `src-tauri/src/panic_hook.rs`.
- One `Cargo.lock` at the root; delete `src-tauri/Cargo.lock` and
  `scripts/icon-tools/Cargo.lock`.
- Target dir moves to `/target`: add it to the root `.gitignore`, update the CI
  cache paths and keys in `ci.yml` and `release.yml`.
- Existing `--manifest-path` invocations keep working.

### 3.2 `cc-switch-contract` crate

A `no_std`-free but dependency-light crate (serde, serde_json, indexmap,
chrono) that owns the types crossing the IPC boundary: `Provider`,
`AppSettings`, `McpServer`, `UsageStats`, `ProxyStatus`, `SkillInfo`, etc.
Today these live inside `cc_switch_lib`, which depends on `tauri` and cannot
compile to wasm32. The move is mechanical: `src-tauri` re-exports them.

The crate also holds:

- `commands::*` constants: one `&str` per command name, grouped like
  `src/lib/api/*.ts`. A `src-tauri` unit test asserts that every constant is
  present in `generate_handler!` and vice versa, so the two sides cannot drift.
- `events::*` constants for the 12 events.
- Typed error: `AppError` currently serializes to a flat `String` and skill
  errors smuggle JSON inside it. The contract crate defines
  `#[derive(Serialize, Deserialize)] struct IpcError { code, message, context, suggestion }`,
  commands migrate to `Result<T, IpcError>`, and
  `src/lib/errors/skillErrorParser.ts` disappears.

Keep `#[serde(rename_all = "camelCase")]` during the transition so the React
app keeps working against the same backend.

### 3.3 `cc-switch-ui` crate

- `dioxus = "0.7"` with the `web` feature, `dioxus-primitives` for headless
  components, Tailwind via `dx`'s built-in watcher (it detects
  `tailwind.css` at the crate root and supports Tailwind v3, which is what the
  repo uses; `tailwind.config.cjs` is reused with its content globs pointed at
  `crates/cc-switch-ui/src/**/*.rs`).
- `src/index.css` is copied verbatim as the base stylesheet: it is 226 lines
  of shadcn HSL tokens, glass helpers, drag-region rules and one container
  query. Nothing in it is React-specific.
- IPC: a small `ipc` module wrapping `window.__TAURI__.core.invoke` and
  `event.listen` through `wasm-bindgen`/`serde-wasm-bindgen`, exposing
  `async fn invoke<T: DeserializeOwned>(cmd: &str, args: &impl Serialize) -> Result<T, IpcError>`.
  `tauri-wasm` 0.2 exists but is small and infrequently updated; writing the
  ~150 lines ourselves avoids a dependency on a third party for the most
  critical path. Requires `app.withGlobalTauri: true` in the Dioxus Tauri
  config.
- API layer: `api::providers`, `api::settings`, ... mirroring the 26 TS
  modules, each function a one-liner calling `invoke` with a contract constant
  and contract types. This is the 273-command surface; it is boring code that
  a checklist can track.
- Data layer replacing react-query: a `Store` context holding `Signal<T>`
  per query key with `use_resource` loaders, explicit `invalidate(key)` and
  event-driven invalidation (`provider-switched` -> providers, current
  provider; `usage-cache-updated` -> usage; and so on). Mutations call the API
  then invalidate. Dioxus 0.7 "Stores" give fine-grained updates for the
  provider map without re-rendering the whole list.
- Navigation: `enum AppId` (8 variants) and `enum View` (14 variants) in two
  signals, persisted to `localStorage` under the same keys the React app uses
  (`cc-switch-last-app`, the view key) so switching UIs preserves state. No
  router crate.
- Theme: same `light | dark | system` logic, same `localStorage` key
  `cc-switch-theme`, same `set_window_theme` invoke to sync the native title
  bar, class toggle on `<html>`.
- Custom title bar: `data-tauri-drag-region` attributes and the
  `core:window:*` permissions are unchanged; window calls go through
  `window.__TAURI__.window`.
- i18n: keep the four JSON files byte-for-byte. A build script embeds them
  (`include_str!` + `serde_json`) into a flat `HashMap<&str, &str>` per
  locale, with a `t!("key.path", name = value)` macro that implements the
  i18next `{{name}}` interpolation subset the app uses. Language selection
  keeps the `localStorage["language"]` fast path plus the `AppSettings.language`
  source of truth. `dioxus-i18n` (Fluent) was considered and rejected: it
  would require converting 10k lines of translations and retraining
  contributors on Fluent syntax for no functional gain.

### 3.4 Dev and build wiring

- Dev: `dx serve --platform web --port 3001` for the Dioxus app; a second
  Tauri config `src-tauri/tauri.dioxus.conf.json` (merged with
  `tauri dev --config`) sets `devUrl` to port 3001 and `beforeDevCommand` to
  `dx serve`. `pnpm dev` keeps launching React; `pnpm dev:dioxus` launches the
  new UI. Both UIs run against the same backend build.
- Release: `dx build --release --platform web` outputs to
  `crates/cc-switch-ui/dist`; the Dioxus config points `frontendDist` there.
  The cutover PR (§6, Phase 6) swaps the default config.
- CSP: WASM needs `'wasm-unsafe-eval'` in `script-src`. Add it in the Dioxus
  config only.
- CI: an `ui-dioxus` job installs `wasm32-unknown-unknown` and `dioxus-cli`
  (pinned), runs `cargo fmt`, `cargo clippy --target wasm32-unknown-unknown`,
  `cargo test` (native, for pure logic), then `dx build --release`.
- Toolchain: pin `dioxus = "0.7"` (0.7.10 is current; 0.8 is alpha). Vendor
  the primitives via `dx components add` so the crate does not depend on the
  unreleased `dioxus-primitives` (crates.io only has 0.0.0).

## 4. Backend-first moves (Phase 1, React still shipping)

These land before any UI code and shrink the port by ~17%. The existing 68
vitest files keep guarding behavior while the React app reads the data from
new commands instead of local modules.

| Today (TS) | Lines | Destination |
|---|---:|---|
| `src/config/*ProviderPresets.ts` (8 catalogs) | ~10,750 | `cc_switch_lib::provider_presets`, serde structs, JSON resources under `src-tauri/src/resources/presets/` embedded at compile time; commands `get_provider_presets(app)`, `get_mcp_presets()`, `get_coding_plan_providers()`. |
| `src/config/iconInference.ts` | 77 | Already exists as `provider_defaults.rs`; expose `infer_provider_icon(name)` and delete the TS copy. |
| `src/config/mcpPresets.ts` (Windows `cmd /c` wrapping) | 104 | Rust, platform logic belongs there. |
| `src/utils/providerConfigUtils.ts`, `tomlUtils.ts`, `textNormalization.ts`, `grokBuildConfig.ts` | ~1,920 | Commands over `toml_edit` (comment-preserving). Code comments at `providerConfigUtils.ts:348` and `lib/api/config.ts:54` already say the merge must not happen in the frontend. |
| `src/lib/version.ts`, `utils/base64.ts`, `utils/uuid.ts`, `lib/platform.ts` | ~213 | `semver`, `base64`, `uuid` crates and `std::env::consts::OS`. |
| `src/hooks/useSessionSearch.ts` (flexsearch) | ~150 | Backend command `search_sessions(query)` over the existing session index; removes the last heavy JS dependency in that area. |
| `src/lib/schemas/*.ts` (zod) | 280 | Validation returns from Rust as `IpcError`s with field paths. |

Cleanups folded into Phase 1: drop the 12 never-invoked commands, the dead
`configLoadError` listener in `main.tsx`, the unlistened `proxy-flags-changed`
emits in `tray.rs`, the unused `jsonc-parser` dependency, and rename the two
camelCase commands (`queryProviderUsage`, `testUsageScript`) to snake_case.

## 5. Library replacement decisions

| React dependency | Used in | Decision |
|---|---|---|
| Radix (7 portal/floating primitives), shadcn `ui/` | everywhere | `dioxus-primitives` (dialog, dropdown menu, select, popover, tooltip, tabs, accordion, checkbox, switch, scroll area, toast, ...), vendored with `dx components add`, styled with the existing Tailwind tokens. |
| sonner toasts | 67 files | primitives `toast`; a `use_toast()` hook with the same `success/error/info` API so call sites port 1:1. |
| react-hook-form + zod | 4 forms + `ui/form.tsx` | Plain signals per field; validation errors come from Rust (§4). |
| @tanstack/react-query | 37 files | Custom store (§3.3). |
| framer-motion | 11 files, fades only | CSS transitions/keyframes already defined in `tailwind.config.cjs`. `dioxus-motion` only if a case needs it. |
| CodeMirror 6 | `JsonEditor`, `MarkdownEditor` | Keep CodeMirror through JS interop: a ~5 KB `editor.js` asset bundling the CM6 modules, mounted from Rust via `document::eval`/`web-sys`, value changes delivered by a JS->Rust callback. There is no Rust editor with comparable JSON/Markdown linting; a plain `<textarea>` fallback is provided behind a flag for parity testing. This is the only JS kept in the app. |
| @dnd-kit | provider list reorder | Pointer-event reorder written in Rust (one list, vertical only). |
| recharts | `UsageTrendChart` | Inline SVG chart component (one line/area chart with tooltip). |
| @tanstack/react-virtual | session list | Simple window-based virtualization in Rust (one list). |
| cmdk | `ui/command.tsx` | primitives combobox / hand-written filtered list. |
| flexsearch | session search | Moved to backend (§4). |
| lucide-react | app-wide icons | `dioxus-free-icons` (Lucide set) or an `icons.rs` generated from the SVGs by extending `scripts/icon-tools` (`index` already renders inline SVG maps; add a `--rust` output). |
| i18next | app-wide | Embedded JSON + `t!` macro (§3.3). |
| smol-toml, jsonc-parser, json5 | config utils | Deleted; backend owns parsing. |

## 6. Phases and deliverables

Each phase is one or more PRs against `main`. The React UI stays the default
until Phase 6. Sizes are rough and expressed in lines of Rust to write.

| Phase | Deliverable | Size | Exit criterion |
|---|---|---:|---|
| 0. Skeleton | Root workspace, `cc-switch-contract` with the first 10 types, `cc-switch-ui` hello-world that invokes `get_settings` and renders the settings JSON, `tauri.dioxus.conf.json`, `pnpm dev:dioxus`, CI job. | ~1.5k | `pnpm dev:dioxus` opens a Tauri window showing live backend data; CI green. |
| 1. Backend-first moves | §4 in full; React app switched to the new commands. | ~4k Rust, -13k TS | All 68 vitest files and `cargo test` pass; React app unchanged for users. |
| 2. Foundation | IPC + API layer for all 273 commands, store, events, i18n, theme, title bar, navigation enums, toast, vendored primitives, app shell with an empty view per `View`. | ~8k | Every command has a typed wrapper and the registry test passes; shell navigates all 14 views. |
| 3. Providers | Provider list/cards/drag reorder, `ProviderForm` and the 9 vendor form sets, preset selector, endpoint speed test, config editors (CodeMirror interop lands here). | ~18k | Manual parity checklist for each vendor form; add/edit/switch/delete round-trips against the real backend. |
| 4. Settings, proxy, usage | 6 settings tabs incl. WebDAV/S3 sync, proxy panel, failover, circuit breaker, usage dashboard with SVG chart, pricing config. | ~10k | Parity checklist. |
| 5. Remaining views | MCP, skills (installed + discovery), prompts, sessions (virtualized list, search via backend), workspace, universal, OpenClaw, Hermes, profiles, deep-link dialogs, first-run and DB-upgrade flows, updater. | ~9k | Parity checklist; deep-link import and DB-upgrade paths exercised. |
| 6. Cutover | Default `tauri.conf.json` points at the Dioxus build; `pnpm dev` uses `dx`; delete `src/`, vitest, Node UI deps (keep `pnpm` only for Tailwind and the CodeMirror asset build, or move Tailwind to the standalone binary and drop Node entirely). Update CONTRIBUTING, CI, release workflow, flatpak manifest. | ~1k, -80k TS | Release build on all 5 release matrix targets; app size and startup time within 10% of v3.17. |

Order within Phase 3 to 5 follows user impact: Claude and Codex provider forms
first, then Gemini, then the rest.

## 7. Testing and validation strategy

- Contract: `cargo test` in `src-tauri` includes the command-registry test
  (§3.2) and serde round-trip tests for every contract type against fixtures
  captured from the current React app (record real `invoke` payloads once with
  a debug hook, store as JSON fixtures).
- Moved logic (Phase 1): unit tests ported from
  `src/utils/*.test.ts` and `tests/config/*` to Rust, one-to-one, before the TS
  is deleted.
- UI components: pure components tested with `dioxus-ssr` render-to-string
  snapshots (no browser). Stateful flows tested with `wasm-bindgen-test` in
  headless Chromium (already available in CI images) against a mock
  `window.__TAURI__` implemented in ~300 lines of JS, ported from the MSW state
  fake in `tests/msw/state.ts`.
- End to end: Playwright against `dx serve` with the same mock, covering the
  flows the two existing integration tests cover (`App.test.tsx`,
  `SettingsDialog.test.tsx`) plus provider add/switch and settings save.
- Manual parity: a checklist per view in `docs/dev/dioxus-parity.md`, ticked
  per phase, screenshots of React vs Dioxus for the 15 largest components.
- Bundle: `dx build --release` size budget of 4 MB WASM (gz ~1 MB); the
  webview loads it locally so this is comfortable, but it is tracked in CI.

## 8. Risks and mitigations

| Risk | Mitigation |
|---|---|
| `dioxus-primitives` is unreleased on crates.io and Dioxus 0.8 is in alpha. | Pin 0.7.x; vendor primitives via `dx components add`; upgrade in a dedicated PR after Phase 6. |
| CodeMirror interop is the only JS left and can break with `dx` asset hashing. | Isolate in one module with a textarea fallback; test the interop in the wasm-bindgen suite. |
| The 22k-line provider forms area hides business rules in 22 hooks. | Port hooks first as plain Rust functions with unit tests, then the rsx. |
| Two UIs in the tree for several weeks confuse contributors. | `docs/dev/` explains which is default; CI runs both; Phase 6 is a hard deadline once parity checklists are complete. |
| WASM cold start slower than the JS bundle. | Measure at Phase 0; `wasm-opt` via `dx` release profile; lazy-load the sessions and usage views with `wasm-split` if needed. |
| Backend `AppError` messages are Chinese-only strings surfaced to the UI. | `IpcError` carries a key so the UI localizes; `Localized` variant already exists to build on. |

## 9. Immediate next step

Phase 0 in a single PR: root workspace with hoisted profile and CI/gitignore
updates, `crates/cc-switch-contract` seeded with `AppSettings` and `Provider`,
`crates/cc-switch-ui` rendering `get_settings` output inside a Tauri window
via `tauri.dioxus.conf.json`, and the CI job. That proves the toolchain
(wasm32 target, `dx`, CSP, `withGlobalTauri`, Tailwind watcher) before any
feature work starts.
