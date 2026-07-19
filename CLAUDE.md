# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

CC Switch is a cross-platform Tauri 2 desktop app that manages "providers" (API endpoint + key configurations) for eight AI coding tools — Claude Code, Claude Desktop, Codex, Gemini CLI, Grok Build, OpenCode, OpenClaw, and Hermes — and switches which one is live by writing each tool's own config files (`~/.claude/settings.json`, `~/.codex/config.toml`, etc.). It also provides unified MCP server / prompts / skills management, a local routing proxy with failover, usage tracking, and a session browser.

React 18 + TypeScript frontend at the repo root (`src/`), Rust backend in `src-tauri/`. Code comments are largely in Chinese.

## Commands

JS commands run from the repo root; Rust commands from `src-tauri/`.

```bash
pnpm install                 # install deps (CI uses --frozen-lockfile)
pnpm dev                     # full app (tauri dev; renderer on http://localhost:3000)
pnpm dev:renderer            # vite only, no Rust backend
pnpm build                   # full tauri build
pnpm typecheck               # tsc --noEmit
pnpm format                  # prettier --write over src/**
pnpm format:check
pnpm test:unit               # vitest run (all JS tests)
pnpm test:unit tests/components/ProviderList.test.tsx   # single file
pnpm test:unit -t "name substring"                      # single case
pnpm test:unit:watch
```

```bash
# in src-tauri/
cargo fmt --check
cargo clippy -- -D warnings  # CI treats clippy warnings as errors
cargo test                   # all Rust tests
cargo test --test provider_commands          # one integration test file
cargo test some_test_fn_name                 # one test by name
```

CI (`.github/workflows/ci.yml`) enforces exactly: `pnpm typecheck && pnpm format:check && pnpm test:unit` (frontend) and `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test` (backend, on Linux/Windows/macOS). There are no git hooks; CI is the only gate.

There is **no lint script and no ESLint config** — CONTRIBUTING.md's mention of `pnpm lint` is stale. Prettier and clippy are the format/lint gates.

Toolchain: pnpm 10.x (CI pins 10.12.3), Node 20+ (`.node-version` says 22.12.0), Rust toolchain 1.95 via `rust-toolchain.toml` (MSRV 1.85).

## Architecture

Data flow: React components → typed API wrappers (`src/lib/api/*`) → Tauri `invoke` → `#[tauri::command]` wrappers (`src-tauri/src/commands/*`) → stateless services (`src-tauri/src/services/*`) → DAOs (`src-tauri/src/database/dao/*`) → SQLite.

### Storage (Rust side)

- **SQLite is the single source of truth**: `~/.cc-switch/cc-switch.db` via rusqlite (`src-tauri/src/database/`). Schema versioned with stepwise migrations in `database/schema.rs`; tables for providers, endpoints, MCP servers, prompts, skills, proxy config/logs, pricing, profiles, settings KV.
- **Device-local settings** live in `~/.cc-switch/settings.json` (`settings.rs`), *not* in the DB: tray/window prefs, per-app config-dir overrides, per-device current-provider overrides. `get_effective_current_provider` prefers local settings over the DB `is_current` flag.
- The app data dir `~/.cc-switch` is overridable via a Tauri Store file (`app_store.rs`). A legacy JSON config model (`app_config.rs`, `MultiAppConfig`) remains for one-time JSON→SQLite migration and import/export.
- File-write discipline (`config.rs`): `atomic_write` (temp + rename), `write_json_file` sorts keys recursively for deterministic output.

### Provider switching (core domain concept)

`services/provider/mod.rs` (`ProviderService::switch`) + `services/provider/live.rs`. Two modes, keyed off `AppType::is_additive_mode()`:

- **Switch mode** (Claude, Codex, Gemini, Grok Build, Claude Desktop): only the current provider's config is written to the tool's live files. Before switching, live-file edits are **backfilled** into the previous provider's DB record, with shareable bits extracted into a per-app "common config snippet" (DB settings table) that is merged into every live write.
- **Additive mode** (OpenCode, OpenClaw, Hermes): all providers coexist in one live file; switching just updates the default model/provider entries.

Live targets per app are resolved in `config.rs` / `<app>_config.rs` (e.g. Claude → `~/.claude/settings.json`, Codex → `~/.codex/auth.json` + `config.toml`, Gemini → `~/.gemini/.env` + `settings.json`). After every live rewrite for an app, enabled MCP servers must be re-projected (Codex's `config.toml` is replaced whole, so `[mcp_servers]` would otherwise be lost).

### Proxy takeover

`services/proxy.rs` runs a local Axum proxy (default `127.0.0.1:15721`) that can "take over" an app: live config is rewritten to point at the proxy with `PROXY_MANAGED` placeholder tokens while the real config is backed up in the DB (`proxy_live_backup` table). Restore paths exist for stop/exit/crash recovery. While takeover is active, provider switching becomes a routing hot-switch (no live-file rewrite) and shares a per-app switch lock with normal switching. `proxy/` contains the protocol adapters (Anthropic ↔ OpenAI Chat/Responses ↔ Gemini), failover/circuit breaker, and usage capture into `proxy_request_logs`.

### MCP / prompts / skills

All are DB-backed SSOT projected into each CLI's own files: MCP via `services/mcp.rs` + `mcp/<app>.rs` (e.g. Claude → `~/.claude.json` `mcpServers`, Codex → toml_edit into `config.toml`); prompts via `services/prompt.rs` → `CLAUDE.md`/`AGENTS.md`/`GEMINI.md`; skills via `services/skill.rs` (symlink or copy from `~/.cc-switch/skills/`).

### Adding a new Tauri command end-to-end

1. Business logic as a method on a service in `src-tauri/src/services/<domain>.rs`; DB access via a DAO method in `src-tauri/src/database/dao/`.
2. `#[tauri::command]` wrapper in `src-tauri/src/commands/<domain>.rs` taking `state: State<'_, AppState>`, parsing `app: String` via `AppType::from_str`, returning `Result<T, String>` (errors converted with `.map_err(|e| e.to_string())`; `AppError` in `error.rs` has a `Localized` variant for bilingual user-facing messages).
3. Declare the module + `pub use` in `src-tauri/src/commands/mod.rs`.
4. Register the command in the `tauri::generate_handler![]` list in `src-tauri/src/lib.rs`.
5. Frontend: add a typed method to the matching module in `src/lib/api/` (`invoke("snake_case_command", { camelCaseArgs })`), then a react-query hook in `src/lib/query/` or `src/hooks/`.

`src-tauri/src/lib.rs` (~2100 lines) is the whole bootstrap: plugin setup, JSON→SQLite migration, DB init with recovery dialogs, seeding, deep-link (`ccswitch://`) handling, tray creation, sync workers, and the ~300-command invoke handler.

### Frontend

- **No router**: `src/App.tsx` holds a `View` string-union state + `activeApp: AppId` (both persisted to localStorage) and a `renderContent()` switch. New views extend the `View` union, `VALID_VIEWS`, and get a `renderContent()` case; panels expose imperative APIs via refs.
- **Server state**: TanStack react-query v5. Query hooks + key factories in `src/lib/query/` (e.g. `useProvidersQuery` keyed `["providers", appId]`); mutations invalidate caches, toast via sonner + `extractErrorMessage` (`src/utils/errorUtils.ts`), and call `providersApi.updateTrayMenu()` on success so the tray stays in sync. No Redux/Zustand; only Theme and Update contexts.
- **Backend events**: Tauri events via `useTauriEvent` hook or per-api `on*` helpers; handlers typically invalidate react-query caches.
- **Forms**: react-hook-form + zod v4 (`zodResolver`), schemas in `src/lib/schemas/`.
- **UI**: shadcn/ui-style primitives in `src/components/ui/` (Radix + cva; `components.json` is the shadcn config), feature folders under `src/components/<feature>/`, Tailwind 3 with `.dark`-class dark mode, `cn` helper in `src/lib/utils.ts`.
- **Path alias**: `@/*` → `src/*` (tsconfig + vite + vitest). `@/types` is `src/types.ts`; `@/types/<x>` are separate files in `src/types/`.
- **Static preset data** (provider presets per app, app registry `APP_IDS`/icon map) lives in `src/config/`.

### i18n

Four locales, single flat JSON file each: `src/i18n/locales/{en,ja,zh,zh-TW}.json` (~3000 lines each; note CONTRIBUTING.md cites outdated paths). Any user-facing string change must update **all four** files and go through i18next `t()` — never hardcode UI strings. Default language is zh, fallback en.

## Testing

### JS (vitest, jsdom)

Tests live under top-level `tests/` (not colocated), mirroring source: `tests/components/`, `tests/hooks/`, `tests/lib/`, `tests/utils/`, `tests/config/`, `tests/integration/`. Import source via `@/`.

- **MSW mocks Tauri IPC, not real HTTP**: `tests/msw/tauriMocks.ts` mocks `@tauri-apps/api/core` so `invoke(cmd, payload)` becomes a fetch to `http://tauri.local/<cmd>`, handled by `tests/msw/handlers.ts` against the in-memory fixture store in `tests/msw/state.ts`. Arrange state with its exported mutators (`setProviders`, `setSettings`, …); fire backend events with `emitTauriEvent`.
- Setup (`tests/setupTests.ts`) initializes i18next with **empty resources**, so `t()` returns raw keys — assert on keys/testids, never translated strings. `afterEach` auto-resets fixtures, handlers, and mocks.
- Component tests stub child components with `vi.mock` and wrap renders in a `QueryClientProvider` (`createTestQueryClient()` from `tests/utils/testQueryClient.ts` disables retries). Integration tests (`tests/integration/App.test.tsx`) keep hooks/api real and `await import("@/App")` after the `vi.mock` calls.

### Rust

- Integration tests in `src-tauri/tests/`, one file per area, sharing `support.rs` (via `#[path]`): every test takes the global `test_mutex()`, calls `reset_test_fs()`, and gets an isolated temp `HOME` (`ensure_test_home()`), then builds a real `AppState` with `create_test_state()` and asserts on both DB state and files written under the temp home.
- The command layer is reached through exported `*_test_hook` functions (see `commands/provider.rs`); the `test-hooks` cargo feature is an empty marker gating their docs visibility.
- In-module `#[cfg(test)]` unit tests are widespread; `serial_test` is used for env-mutating tests.

## Conventions

- Conventional Commits (`feat(provider): …`, `fix(tray): …`); branch from `main` as `feat/…` / `fix/…`; open an issue before large features (CONTRIBUTING.md).
- Frontend invoke calls use snake_case command names with camelCase args. `commands/mod.rs` sets `#![allow(non_snake_case)]` because some command/arg names are camelCase (e.g. `addToLive`) to match Tauri's JS-side conversion.
- Non-fatal follow-up failures (MCP re-projection, backfill) are logged as warnings or returned in `SwitchResult.warnings` — they don't fail the switch.
