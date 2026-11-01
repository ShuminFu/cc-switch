# Architecture

A map of the codebase for contributors and coding agents (DOCS-09): where things live, how a
change flows through the layers, and the invariants every change must keep. Commands and the
checks CI runs are in [CONTRIBUTING.md](../../CONTRIBUTING.md) and [testing.md](testing.md);
releases in [RELEASING.md](../../RELEASING.md).

## Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    Frontend (React + TS)                    │
│  ┌─────────────┐  ┌──────────────┐  ┌──────────────────┐    │
│  │ Components  │  │    Hooks     │  │  TanStack Query  │    │
│  │   (UI)      │──│ (Bus. Logic) │──│   (Cache/Sync)   │    │
│  └─────────────┘  └──────────────┘  └──────────────────┘    │
└────────────────────────┬────────────────────────────────────┘
                         │ Tauri IPC (typed: src/lib/bindings.ts)
┌────────────────────────▼────────────────────────────────────┐
│                  Backend (Tauri + Rust)                     │
│  ┌─────────────┐  ┌──────────────┐  ┌──────────────────┐    │
│  │  Commands   │  │   Services   │  │ Adapters / DAO   │    │
│  │ (API Layer) │──│ (Bus. Layer) │──│ (live files, DB) │    │
│  └─────────────┘  └──────────────┘  └──────────────────┘    │
└─────────────────────────────────────────────────────────────┘
          │                     │                     │
   ~/.cc-switch/cc-switch.db    local proxy     ~/.claude, ~/.codex, ~/.gemini, …
   (SQLite, single source       127.0.0.1:15721  (each tool's "live" config files)
    of truth)
```

The app manages eight tools: Claude Code, Claude Desktop, Codex, Gemini CLI, Grok Build,
OpenCode, OpenClaw and Hermes Agent. For each it stores providers, MCP servers, prompts and
skills in SQLite and **projects** the active one into that tool's own config files.

## Repository layout

```
├── src/                        Frontend (React 18 + TypeScript, Vite root)
│   ├── index.html, main.tsx    Entry; App.tsx is the shell
│   ├── app/                    Shell pieces: routes, navigation, title bar, error boundary
│   ├── routes/                 One component per page (providers, MCP, skills, sessions, settings, …)
│   ├── components/             Feature UI (providers/, mcp/, skills/, proxy/, usage/, settings/, ui/ = shadcn)
│   ├── hooks/                  Business logic hooks
│   ├── lib/api/                Thin `invoke` wrappers per domain
│   ├── lib/bindings.ts         Generated typed IPC bindings (tauri-specta; do not edit)
│   ├── lib/query/              TanStack Query keys (keys.ts, lint-enforced), queries, mutations
│   ├── i18n/locales/           en / zh / zh-TW / ja translations
│   ├── config/                 Provider / MCP presets
│   └── icons/extracted/        Provider icons (see scripts/README.md)
├── src-tauri/                  Backend (Rust, Tauri 2)
│   ├── src/lib.rs              App builder: plugins, tray, deep links, command registration
│   ├── src/commands/           #[tauri::command] layer, one file per domain; bindings.rs exports types
│   ├── src/services/           Business logic (provider, mcp, proxy, skill, sync, usage, …)
│   ├── src/adapters/           One AppAdapter per tool + the shared live-file layer
│   ├── src/database/           SQLite connection, schema + migration ledger, DAOs, backups
│   ├── src/proxy/              Local proxy: server, pipeline, providers/ (protocol bridges), usage
│   ├── src/secrets/            Keychain vault, sync credentials, sync encryption envelope
│   ├── src/deeplink/           ccswitch:// parser and importers (v1, signed v2)
│   ├── src/session_manager/    Session history index across tools
│   ├── src/mcp/                MCP sync per tool
│   ├── src/bin/ccs.rs          `ccs` command-line companion over the same database
│   ├── tests/                  Integration tests (support.rs: isolated TestEnv home)
│   └── wix/, icons/, capabilities/   Packaging
├── tests/                      Frontend tests (vitest + MSW; tests/msw/handlers.ts mocks IPC)
├── scripts/                    Repo tooling (see scripts/README.md)
├── docs/                       User manual, guides, release notes, specs, this page
└── .github/                    Workflows, composite actions, templates
```

## How a provider switch flows

1. The UI calls a typed command (`src/lib/bindings.ts` or a `src/lib/api/*` wrapper).
2. `commands/provider.rs` validates arguments and calls `ProviderService`.
3. `ProviderService` updates SQLite (the single source of truth) and asks the tool's
   `AppAdapter` (`adapter_for(app)`) to write the provider into the live config.
4. The adapter goes through `adapters/live_file.rs`: lenient read, backup of the previous
   content to `~/.cc-switch/backups/<app>/`, atomic write, optimistic concurrency check.
5. Events go back to the frontend; TanStack Query invalidates the affected keys.

When the user edits the **active** provider, the reverse happens: the current live file is read
back ("backfill") so manual edits in the tool's own file are not lost.

## Proxy pipeline

`proxy/pipeline/mod.rs` documents the stages: parse → route/select (failover queue, timeout
policy) → per attempt (budget, circuit breaker permit, endpoint, body transform, auth, headers,
send, classify) → respond (protocol transforms, usage logging). Protocol bridges (Anthropic ↔
OpenAI Chat/Responses ↔ Gemini) live in `proxy/providers/` and share a canonical IR in
`proxy/providers/ir/`. The format matrix is in [docs/proxy/format-matrix.md](../proxy/format-matrix.md);
failover semantics for users in the manual,
[4.6 Proxy Semantics](../user-manual/en/4-proxy/4.6-semantics.md).

**Takeover** points a tool's live config at the local proxy: the original live file is stored in
the `proxy_live_backup` table, the key is replaced by the placeholder `PROXY_MANAGED`, and the
original is restored when takeover stops (`services/proxy.rs`).

## Database

- One SQLite file, `~/.cc-switch/cc-switch.db`, behind a mutex-protected connection plus a read
  pool (`database/read_pool.rs`).
- Schema changes go through the migration ledger `MIGRATIONS` in `database/schema.rs`: new
  installs run the frozen baseline DDL plus every registered migration, existing ones run the
  missing `migrate_v{N-1}_to_v{N}` steps. `SCHEMA_VERSION` is derived from the ledger.
- A `db_premigration_` backup is taken before any upgrade; restores and sync downloads are
  staged and verified before they replace the live database (`database/backup.rs`).

## Frontend conventions

- Data comes from TanStack Query; query keys live only in `src/lib/query/keys.ts` (ESLint rule).
- New commands: either typed via specta (`commands/bindings.rs`, then `pnpm gen:bindings`) or a
  plain `invoke` wrapper in `src/lib/api/`. Every command used in tests needs a default handler in
  `tests/msw/handlers.ts` (unmocked invokes fail).
- All user-facing text goes through `t()` with keys in all four locale files (`pnpm i18n:check`).

## Invariants (do not break these)

1. **SQLite is the source of truth.** Live files are a projection; never treat a live file as the
   only copy of user data.
2. **Never overwrite an unparsable live file.** Fail the operation and tell the user.
3. **Back up before replacing** any live file, and write atomically (temp file + rename).
4. **Preserve what you do not own.** Keys, comments and ordering in a tool's config that CC Switch
   did not write must survive a switch (format-preserving writers in `adapters/`).
5. **Device-local state is never synced or restored over** (proxy listen address, takeover state,
   outbound proxy, current project).
6. **Secrets are never logged** at `info` level (deep-link URLs and auth headers are redacted).
7. **Schema changes only via a new migration**, with a test; never edit the baseline DDL.
8. **Cross-platform:** Windows, macOS and Linux (x64 and ARM64) are all shipped; CI builds all of
   them. Path handling must not assume `/` or a case-sensitive file system.

## Tech stack

**Frontend**: React 18 · TypeScript · Vite 7 · Tailwind CSS 3.4 · TanStack Query v5 ·
react-i18next · react-hook-form · zod · shadcn/ui (Radix) · CodeMirror 6 · recharts · @dnd-kit

**Backend**: Tauri 2 · Rust (toolchain pinned in `rust-toolchain.toml`) · tokio · axum/hyper
(proxy) · rustls · rusqlite (bundled SQLite) · serde · tauri-specta ·
tauri-plugin-updater/process/dialog/store/log/deep-link

**Testing**: vitest · MSW · @testing-library/react · cargo test (+ proptest, serial_test)
