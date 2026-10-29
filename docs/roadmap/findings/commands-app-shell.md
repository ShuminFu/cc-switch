# Commands / IPC / app shell — findings (CMD-xx)

Part of the [next-phase proposals](../README.md). 39 findings. Priorities: P1 = 12, P2 = 18, P3 = 9.

## Area summary

The command layer is large (285 `#[tauri::command]` functions across 34 files, 281 registered in `lib.rs`) and mostly thin, but a handful of files carry real business logic: `commands/misc.rs` alone is 5,414 lines (CLI tool lifecycle + cross-platform terminal launching), `commands/provider.rs` holds the per-template usage-query dispatcher, and `commands/usage.rs` runs raw SQL under `lock_conn!`. The "Commands → Services → DAO" rule is honoured for providers/MCP/skills/prompts/profiles but bypassed for usage, settings, proxy config, OpenClaw, Hermes and workspace commands. The app shell (`lib.rs` setup, `tray.rs`) is the riskiest area: it does seconds of synchronous DB/file work before the window is shown, mixes `futures::executor::block_on` with tokio locks in code paths reachable from async commands, and ~97 sync commands do file I/O / SQLite work on the thread Tauri gives sync commands. Security is mostly sound (path-traversal checks on backups and sessions, 0600 settings on Unix, redaction helper exists) but has concrete gaps: deep-link URLs containing API keys are logged at INFO, two commands return raw Copilot bearer tokens to the webview, the CSP allows `connect-src https: http:`, and sync credentials sit in plaintext JSON. Errors cross IPC as flattened strings, so the frontend substring-matches Chinese error text; the event and command contracts are hand-written in TypeScript and already drift (`proxy-flags-changed` is emitted but never listened to; `configLoadError` is listened to but never emitted). Suggested direction for the next phase: (1) a typed IPC contract (tauri-specta) with a structured `AppError` code enum, (2) move the misc/provider/proxy/failover orchestration into services and make `ProxyService`/`ProviderService` async-clean so every command can be `async` without `block_on`, (3) restructure startup into "migrate → show window → background import with progress events", and (4) close the logging/token/CSP security gaps.

## Index

| ID | Title | Type | Priority | Effort | Confidence |
|---|---|---|---|---|---|
| [CMD-01](#cmd-01) | Deep-link URLs carrying API keys are logged in plaintext | security | P1 | S | high |
| [CMD-02](#cmd-02) | `copilot_get_token` / `copilot_get_token_for_account` hand raw bearer tokens to the webview | security | P1 | S | high |
| [CMD-03](#cmd-03) | Overly broad CSP and over-granted capabilities | security | P1 | S | high |
| [CMD-04](#cmd-04) | Sync credentials (WebDAV password, S3 secret) persist in plaintext `settings.json` | security | P1 | M | high |
| [CMD-06](#cmd-06) | `save_settings` is a lost-update race on the process-global settings | bug | P1 | M | medium |
| [CMD-07](#cmd-07) | `futures::executor::block_on` on tokio locks reachable from async commands | bug | P1 | L | medium |
| [CMD-08](#cmd-08) | ~97 sync commands do file I/O, SQLite aggregation and process spawning on Tauri's sync-command thread | performance | P1 | L | high |
| [CMD-09](#cmd-09) | Startup does seconds of synchronous work before the window is shown | performance | P1 | L | high |
| [CMD-10](#cmd-10) | ~3,500 lines of business logic live in the command layer (and the tray calls commands) | refactor | P1 | XL | high |
| [CMD-12](#cmd-12) | Errors cross IPC as flattened strings; frontend substring-matches Chinese text | refactor | P1 | L | high |
| [CMD-13](#cmd-13) | No typed IPC contract — 281 commands mirrored by hand in 24 TS files with inconsistent naming | refactor | P1 | XL | high |
| [CMD-31](#cmd-31) | Command/shell test coverage is thin and the test harness is awkward | test | P1 | L | high |
| [CMD-05](#cmd-05) | `open_provider_terminal` writes the provider API key to a world-readable temp file | security | P2 | S | high |
| [CMD-11](#cmd-11) | Raw SQL and direct DAO calls inside commands bypass the service layer | refactor | P2 | M | high |
| [CMD-14](#cmd-14) | Event contract is untyped string literals and already drifts | refactor | P2 | M | high |
| [CMD-15](#cmd-15) | `show_in_tray` setting is exposed in the UI but never read by the backend | bug | P2 | S | high |
| [CMD-16](#cmd-16) | Tray menu is fully rebuilt (≥6 DB queries + `block_on`) at 13 call sites, with four copies of the rebuild code | performance | P2 | M | high |
| [CMD-18](#cmd-18) | Per-app enumeration is hand-rolled in ~15 places; adding an app type touches all of them | refactor | P2 | L | high |
| [CMD-19](#cmd-19) | Throwaway second `AppState`/`ProxyService` constructed inside commands | bug | P2 | S | medium |
| [CMD-20](#cmd-20) | Legacy / duplicate / stub commands remain registered | tech-debt | P2 | M | high |
| [CMD-22](#cmd-22) | `async` commands that block the runtime without `spawn_blocking` | performance | P2 | M | high |
| [CMD-23](#cmd-23) | Input-validation gaps on IPC parameters | security | P2 | M | medium |
| [CMD-24](#cmd-24) | macOS deep links are handled twice (plugin listener + manual `RunEvent::Opened` arm) | bug | P2 | S | low |
| [CMD-25](#cmd-25) | Multi-step command writes are non-transactional | bug | P2 | M | medium |
| [CMD-29](#cmd-29) | Crash handling and logging retention lose the context you need after a crash | feature | P2 | M | high |
| [CMD-30](#cmd-30) | No command-level tracing, timing or request ids | feature | P2 | M | high |
| [CMD-32](#cmd-32) | No developer documentation for the IPC surface, events, startup sequence or security posture | docs | P2 | M | high |
| [CMD-33](#cmd-33) | Feature — headless / CLI mode reusing the service layer | feature | P2 | L | medium |
| [CMD-36](#cmd-36) | Exit/restart paths rely on `std::process::exit` after fixed sleeps | tech-debt | P2 | M | medium |
| [CMD-37](#cmd-37) | Window recreated by lightweight mode loses Linux/decoration/theme setup | bug | P2 | S | medium |
| [CMD-17](#cmd-17) | Two locale detectors and a second hard-coded i18n table in Rust | tech-debt | P3 | S | high |
| [CMD-21](#cmd-21) | Commands load the whole provider table to find one row; settings cloned per call | performance | P3 | S | high |
| [CMD-26](#cmd-26) | `stream_check_all_providers` probes sequentially | performance | P3 | S | high |
| [CMD-27](#cmd-27) | `webdav_sync.rs` and `s3_sync.rs` are copy-paste twins returning untyped JSON | refactor | P3 | M | high |
| [CMD-28](#cmd-28) | Two ad-hoc thread-sleeping debouncers and two "global AppHandle" patterns | tech-debt | P3 | S | high |
| [CMD-34](#cmd-34) | Feature — global hotkeys and a second (quick-switcher) window | feature | P3 | M | medium |
| [CMD-35](#cmd-35) | Feature — extension API for provider adapters and usage scripts | feature | P3 | XL | low |
| [CMD-38](#cmd-38) | `set_app_config_dir_override` accepts unvalidated paths and silently falls back later | bug | P3 | S | high |
| [CMD-39](#cmd-39) | Updater has three overlapping entry points and the frontend calls the plugin directly | tech-debt | P3 | S | high |

## Findings

### CMD-01: Deep-link URLs carrying API keys are logged in plaintext

**Type:** security · **Priority:** P1 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/commands/deeplink.rs:11` — `log::info!("Parsing deep link URL: {url}")` logs the raw URL at INFO; `src-tauri/src/lib.rs:143` — `log::debug!("Deep link URL (raw) from {source}: {url_str}")`; `src-tauri/src/lib.rs:1614` — macOS `RunEvent::Opened` arm logs `url_str` raw at INFO; `src-tauri/src/deeplink/provider.rs:44-48` — provider deep links carry `api_key` (and `usage_api_key`, 197-209); `src-tauri/src/lib.rs:353-372` — the file logger is initialised at `LevelFilter::Trace` and only lowered at 968-979 after the DB loads; `src-tauri/src/lib.rs:95-124,142` — a `redact_url_for_log` helper exists and is used on the adjacent line.
- **Problem:** Any `ccswitch://…?api_key=…` import writes the secret into `~/.cc-switch/logs/cc-switch.log` (1 GB cap, `RotationStrategy::KeepSome(2)`), on three code paths, two of them at INFO level.
- **Proposal:** Use `redact_url_for_log` on all three sites; delete the raw DEBUG line; add a `Debug` impl for `DeepLinkImportRequest` that masks `api_key`/`usage_api_key`; add a unit test that greps log output for the key after `parse_deeplink`.
- **Acceptance:** A test that imports a deep link with a sentinel key asserts the sentinel never appears in the log sink; `rg "log::.*url_str|log::.*\{url\}" src-tauri/src` returns only redacted calls.

### CMD-02: `copilot_get_token` / `copilot_get_token_for_account` hand raw bearer tokens to the webview

**Type:** security · **Priority:** P1 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/commands/copilot.rs:152-175` — both commands return `Result<String, String>` containing the Copilot access token, with the doc comment "内部使用，用于代理请求" (internal, for proxy requests); registered at `src-tauri/src/lib.rs:1498-1499`; `src/lib/api/copilot.ts:120-122,226-230` — TS wrappers exist but have no callers anywhere in `src/` (grep for `copilotGetToken` / `copilotGetTokenForAccount` outside the api file returns nothing).
- **Problem:** The proxy already resolves tokens server-side; exposing them over IPC gives any webview compromise (see CMD-03) a one-call token exfiltration primitive, for a capability the UI never uses.
- **Proposal:** Delete both commands and their TS wrappers; if the UI ever needs "is token valid", expose a boolean/expiry-only command.
- **Acceptance:** Commands removed from `generate_handler!`; `rg copilot_get_token src src-tauri` is empty; auth flows still pass existing tests.

### CMD-03: Overly broad CSP and over-granted capabilities

**Type:** security · **Priority:** P1 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/tauri.conf.json:29` — `connect-src 'self' ipc: http://ipc.localhost https: http:` and `img-src 'self' data: https: http:`, `style-src 'unsafe-inline'`; `src-tauri/capabilities/default.json:9-11,20` — grants `opener:default`, `updater:default`, `dialog:default` to the `main` window; the frontend imports only `@tauri-apps/plugin-dialog`, `plugin-process`, `plugin-updater` (0 imports of `plugin-opener`), and all URL/folder opening goes through backend commands (`src-tauri/src/commands/misc.rs:22-35`, `config.rs:164-189`); `assetProtocol.enable: true` with `scope: []` (`tauri.conf.json:30-33`).
- **Problem:** With `connect-src http: https:` the webview can POST anywhere, which turns an XSS (e.g. via provider names, MCP JSON, deep-link payloads rendered in the UI) into data exfiltration, and `opener:default` lets injected JS open arbitrary URLs/paths without going through the Rust allow-list.
- **Proposal:** Restrict `connect-src` to `'self' ipc: http://ipc.localhost` (the backend performs all network calls; the only direct frontend network use is the updater, which can be routed through `install_update_and_restart`); drop `opener:default`; drop `updater:default` once `src/lib/updater.ts:31` is replaced by the existing backend commands; remove `unsafe-inline` for styles if Tailwind build allows (or scope via nonce); disable `assetProtocol` or give it a real scope.
- **Acceptance:** `pnpm build` + manual smoke of settings/updater/open-folder flows pass with the tightened CSP; a `tauri.conf.json` CI lint asserts `connect-src` contains no bare `http:`/`https:`.

### CMD-04: Sync credentials (WebDAV password, S3 secret) persist in plaintext `settings.json`

**Type:** security · **Priority:** P1 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/settings.rs:110-127,185-206` — `password` / `secret_access_key` are plain `String` fields of `AppSettings`; `settings.rs:664-700` — written as pretty JSON, mode 0600 on Unix only, plain `fs::write` on Windows (694-697); `src-tauri/src/commands/settings.rs:13-52` — `merge_settings_for_save` treats an empty incoming password as "keep existing", which also makes it impossible to clear a stored password via `save_settings`.
- **Problem:** Secrets at rest in a user-readable JSON (no ACL on Windows), copied by any backup/sync of `~/.cc-switch`, and the "empty means keep" hack is an API footgun (tests at 376-442 enshrine it).
- **Proposal:** Store sync credentials in the OS keychain (`keyring` crate or `tauri-plugin-stronghold`), keep only a reference in settings; replace the merge hack with an explicit `password: Option<PasswordUpdate>` (`Keep | Set(String) | Clear`) on the save command; apply the same to any future provider-level secrets.
- **Acceptance:** `settings.json` contains no credential strings after configuring WebDAV/S3; a test asserts `Clear` removes the secret; Windows build writes nothing sensitive to disk.

### CMD-06: `save_settings` is a lost-update race on the process-global settings

**Type:** bug · **Priority:** P1 · **Effort:** M · **Confidence:** medium

- **Evidence:** `src-tauri/src/commands/settings.rs:66-71` — `let existing = get_settings(); let merged = merge…; update_settings(merged)`; `src-tauri/src/settings.rs:748-758` — `update_settings` writes the file *then* takes the write lock, so the read and the write are not atomic; `settings.rs:760-774` — `mutate_settings` (used by `set_current_provider` 980-992, sync status updates 1118-1124, migration markers) is the only lock-held read-modify-write; `settings.rs:683-697` — the file is truncated and rewritten in place (no temp+rename), and `load_from_file` (639-661) silently falls back to defaults on a parse error.
- **Problem:** A `save_settings` from the UI concurrent with a provider switch (which calls `set_current_provider`) or an auto-sync status update can overwrite the other's fields with stale values; a crash mid-write corrupts `settings.json` and all device settings silently reset.
- **Proposal:** Make `update_settings` take the write lock before reading/merging (or move merge into `mutate_settings`), write via temp file + `rename`, and expose a `PATCH`-style `update_settings_partial(fields)` command so the UI stops sending the whole struct; log loudly (and keep a `.bak`) when the file fails to parse.
- **Acceptance:** A test spawning concurrent `save_settings` + `set_current_provider` ends with both changes present; a test that corrupts `settings.json` yields a `.bak` and a warning instead of silent defaults.

### CMD-07: `futures::executor::block_on` on tokio locks reachable from async commands

**Type:** bug · **Priority:** P1 · **Effort:** L · **Confidence:** medium

- **Evidence:** `src-tauri/src/tray.rs:675,712` — `create_tray_menu` calls `block_on(app_state.proxy_service.is_running())` and `block_on(db.get_live_backup(..))`; `src-tauri/src/services/proxy.rs:3126-3128` — `is_running` awaits a tokio `RwLock`; `create_tray_menu` is invoked from the async command `set_auto_failover_enabled` (`src-tauri/src/commands/failover.rs:175`), the async command `update_tray_menu` (`src-tauri/src/lib.rs:193-211`) and from a `tauri::async_runtime::spawn` task via `emit_profile_apply_events` → `refresh_tray_menu` (`src-tauri/src/commands/profile.rs:182-192`, `tray.rs:908-920`); `src-tauri/src/services/provider/mod.rs:2298-2343` — the provider update/switch path has five `block_on` calls into `ProxyService`, and is reached from the async command `set_common_config_snippet` (`src-tauri/src/commands/config.rs:346-353`); `src-tauri/src/commands/profile.rs:161-164` — a comment admits `ProviderService::switch` "uses block_on internally; putting it in an async command would panic on the runtime thread"; 36 `block_on` sites total in `src-tauri/src` (tray.rs ×6, services/proxy.rs ×7, services/provider ×14).
- **Problem:** Blocking a tokio worker on a tokio lock that may be held by a task scheduled on the same worker is a latent deadlock (and `block_on` inside `#[tokio::main]`-style contexts panics in some configurations, as the profile.rs comment shows the team already hit). It also forces commands to stay sync (CMD-08).
- **Proposal:** Make the "proxy running / takeover state" readable without awaiting (an `AtomicBool`/`watch::Receiver` snapshot on `ProxyService`), convert `ProviderService::switch`/`update` and `ProxyService::*_sync` to `async fn` with sync callers using `spawn_blocking` + `Handle::block_on` only at the outermost edge (tray click handlers, exit path), and add a clippy-level lint/CI grep that forbids `futures::executor::block_on` outside `services/proxy.rs` shutdown helpers.
- **Acceptance:** `rg "futures::executor::block_on" src-tauri/src/tray.rs src-tauri/src/services/provider` returns nothing; `apply_profile`/`switch_provider` become `async` commands; a stress test toggles failover while rebuilding the tray 1,000× without hang.

### CMD-08: ~97 sync commands do file I/O, SQLite aggregation and process spawning on Tauri's sync-command thread

**Type:** performance · **Priority:** P1 · **Effort:** L · **Confidence:** high

- **Evidence:** Census: 97 `#[tauri::command] pub fn` vs 185 `pub async fn`. Sync examples: `src-tauri/src/commands/provider.rs:23-110` (`get_providers`, `switch_provider` → live file writes + DB), `src-tauri/src/commands/usage.rs:12-121,278-324` (usage aggregations; `sync_session_usage` scans every Claude/Codex/Gemini/OpenCode session log), `src-tauri/src/commands/skill.rs:327-336` (`install_skills_from_zip` extracts an archive), `src-tauri/src/commands/openclaw.rs` (14 sync commands rewriting `openclaw.json`), `src-tauri/src/commands/hermes.rs:22-79`, `src-tauri/src/commands/import_export.rs:146-176`, `src-tauri/src/commands/deeplink.rs:27-42`. Tauri 2 executes non-`async` commands on the main thread unless annotated `#[tauri::command(async)]`; the only reason given for staying sync is the `block_on` constraint (`commands/profile.rs:161-164`, CMD-07).
- **Problem:** Every provider switch, usage dashboard query or skill install blocks the UI/event loop for the duration of the file/DB work; the usage dashboard's six sync aggregation commands run on the same thread as window events.
- **Proposal:** After CMD-07, mark remaining sync commands `#[tauri::command(async)]` (or make them `async fn` wrapping `spawn_blocking`); for DB-heavy commands introduce a `db.run_blocking(|conn| …)` helper on `Database` that uses `spawn_blocking`; extract the shared 4-app session sync into `services::session_usage::sync_all(db)` (currently duplicated at `lib.rs:1105-1148` and `usage.rs:278-324`).
- **Acceptance:** `rg -U "#\[tauri::command\]\n\s*pub fn" src-tauri/src/commands` returns zero; UI stays responsive (tested by switching providers while dragging the window) on a DB with 100k `proxy_request_logs` rows.

### CMD-09: Startup does seconds of synchronous work before the window is shown

**Type:** performance · **Priority:** P1 · **Effort:** L · **Confidence:** high

- **Evidence:** `src-tauri/src/lib.rs:318-1200` — the whole `setup` closure runs on the main thread before `window.show()` at 1169-1196. Inside it: `Database::init()` (452-467 → `src-tauri/src/database/mod.rs:99-163`: `create_tables`, pre-migration file backup, `apply_schema_migrations`, `ensure_model_pricing_seeded`, `cleanup_old_stream_check_logs(7)`, `rollup_and_prune(30)` over `proxy_request_logs`, `PRAGMA incremental_vacuum`), skills SSOT migration (503-550), per-app live import + official seeding (566-607), three additive imports that run on *every* start (687-707), two OMO imports (711-758), MCP/prompt imports (761-837), tray menu build with ≥6 DB queries + `block_on` (911), and the JSON→SQLite migration loop with blocking dialogs (391-416). Only Codex history migration (609-669) and the proxy-state restore (1041-1151) are deferred.
- **Problem:** Cold start on a large usage DB (rollup + vacuum) or slow disk shows nothing for seconds; silent-startup users (1175-1182) notice nothing but deep-link/single-instance callbacks arriving during setup target a window that is not yet shown; there is no progress signal to the frontend.
- **Proposal:** Split startup into (a) *critical path*: config-dir override, logging, DB open + schema migration, `AppState` manage, window show; (b) *background init task* emitting `init-progress`/`init-complete` events for imports, seeding, rollup/vacuum, tray build; make the frontend render a loading shell until `init-complete`; move `rollup_and_prune`/`incremental_vacuum` to the existing daily maintenance timer (1075-1089); make the "every start" additive imports (687-707) conditional on file mtime.
- **Acceptance:** Time-to-first-paint measured via a `startup-timing` log line is < 500 ms on a 100k-row DB; all imports still run (existing `provider_commands.rs` startup tests pass); `get_init_error` still works for the DB-too-new path (426-450).

### CMD-10: ~3,500 lines of business logic live in the command layer (and the tray calls commands)

**Type:** refactor · **Priority:** P1 · **Effort:** XL · **Confidence:** high

- **Evidence:** `src-tauri/src/commands/misc.rs` is 5,414 lines with 12 commands: CLI tool version detection/upgrade planning (`100-2560`: npm/brew/volta/bun/mise/WSL probing, semver, install-command synthesis) and cross-platform terminal launching (`2601-3520`: AppleScript builders, iTerm/Warp/Ghostty/Kitty, Windows `.bat` generation); `src-tauri/src/commands/provider.rs:463-672` — `query_provider_usage_inner` dispatches Copilot / coding-plan / balance / subscription / JS-script usage queries and shapes `UsageResult`; `provider.rs:169-379` — Claude Desktop import + route inference; `src-tauri/src/commands/proxy.rs:329-409` — `reset_circuit_breaker` implements failover switch-back; `src-tauri/src/commands/failover.rs:79-182` — enable-failover workflow, duplicated with subtle differences in `src-tauri/src/tray.rs:504-590` (`handle_auto_click`); `src-tauri/src/commands/config.rs:306-382` — snippet save + legacy migration + live sync + OMO write; `src-tauri/src/commands/workspace.rs` — entire OpenClaw memory file store incl. full-text search (194-285). Inverted dependencies: `tray.rs:1103` calls the `#[tauri::command]` `commands::queryProviderUsage` with hand-built `State`s; `tray.rs:444,453` call `commands::emit_profile_apply_events`; `commands/hermes.rs:139` calls `commands::misc::launch_terminal_running`; `services/mod.rs` has no tool-lifecycle, terminal, failover, usage-query or tray service.
- **Problem:** The logic is untestable without Tauri `State`, is duplicated between UI and tray entry points (failover enable, tray rebuild), and the "Commands → Services → DAO" rule stated in the architecture is not enforceable while commands are the only home for these workflows.
- **Proposal:** Create `services/tool_lifecycle.rs`, `services/terminal.rs`, `services/usage_query.rs`, `services/failover.rs` (single `enable(app, source)` used by both `set_auto_failover_enabled` and `handle_auto_click`), `services/claude_desktop_import.rs`, `services/workspace.rs`; reduce every command to parse → call service → map error; forbid `use crate::commands::` outside `commands/` and `lib.rs` via a CI grep; cap command files at ~300 lines.
- **Acceptance:** `commands/misc.rs` < 300 lines; `rg "crate::commands::" src-tauri/src --glob '!src-tauri/src/commands/**' --glob '!src-tauri/src/lib.rs'` is empty; the moved logic gains service-level tests (the 180 unit tests at `misc.rs:3537-5414` move with it).

### CMD-12: Errors cross IPC as flattened strings; frontend substring-matches Chinese text

**Type:** refactor · **Priority:** P1 · **Effort:** L · **Confidence:** high

- **Evidence:** Return types: ~300 `Result<_, String>` vs 38 `Result<_, AppError>` across `commands/` (`usage.rs`, `stream_check.rs` use `AppError`; everything else `.map_err(|e| e.to_string())`); `src-tauri/src/error.rs:108-121` — `From<AppError> for String` and `Serialize` both emit `to_string()`, so `AppError::Localized { key, zh, en }` arrives as `"zh (en)"` and the `key` is dropped; `src-tauri/src/error.rs:124-146` — `format_skill_error` encodes JSON *inside* the error string as a workaround; `src-tauri/src/commands/config.rs:20-42` — ad-hoc zh/en/ja messages chosen by reading settings in the command; `src-tauri/src/commands/hermes.rs:8-11` — a string constant "kept in sync with the frontend"; `src/utils/errorUtils.ts:45-108` — `translateMcpBackendError` matches `msg.includes("MCP 服务器 ID 不能为空")` etc. to pick i18n keys.
- **Problem:** Any wording change in Rust silently breaks frontend i18n; the UI cannot distinguish invalid-input from I/O from "not found"; bilingual concatenated strings leak to users.
- **Proposal:** Serialize `AppError` as `{ code: &'static str, message: String, details?: Value }` (a `#[serde(tag="code")]` enum or a small `ErrorDto`); return `Result<T, AppError>` from every command (clippy-enforced); give `Localized` a stable `code` and let the frontend map `code → t(key)`; delete `translateMcpBackendError`, `format_skill_error`, and the per-command zh/en/ja branches; expose the error type through the typed contract (CMD-13).
- **Acceptance:** `rg "Result<[^,]+, String>" src-tauri/src/commands` is empty; `errorUtils.ts` has no Chinese literals; a Vitest asserts that every backend `code` has an i18n key.

### CMD-13: No typed IPC contract — 281 commands mirrored by hand in 24 TS files with inconsistent naming

**Type:** refactor · **Priority:** P1 · **Effort:** XL · **Confidence:** high

- **Evidence:** `src/lib/api/*.ts` (24 files, 291 `invoke` callsites) hand-declare interfaces such as `src/lib/api/providers.ts:10-40` that mirror Rust structs; no `specta`/`tauri-specta` dependency in `src-tauri/Cargo.toml`; parameter naming is mixed — `app` (`commands/provider.rs:25`), `app_type` (`commands/config.rs:280`), `appType` (`config.rs:407`), `#[allow(non_snake_case)] providerId` (`provider.rs:42,54,387`), `#[tauri::command(rename_all = "camelCase")]` on some (`auth.rs:81`, `copilot.rs:37`) but not neighbours (`copilot.rs:21`); `commands/mod.rs:1` and six command files carry a file-level `#![allow(non_snake_case)]` to tolerate this; return payloads are sometimes untyped `serde_json::Value` (`import_export.rs:21-76`, `webdav_sync.rs:84-169`, `deeplink.rs:46-89`).
- **Problem:** Renames and struct changes are caught only at runtime; the diff of registered vs. invoked names already shows 15 registered commands with no literal `invoke` callsite (`get_claude_config_status`, `get_claude_plugin_status`, `read_claude_plugin_config`, `is_claude_plugin_applied`, `get_copilot_optimizer_config`, `set_copilot_optimizer_config`, `get_hermes_live_provider`, `import_from_deeplink`, `check_app_update_available`, lightweight ×3, `get_init_error`, `get_skills_migration_result`, `get_config_status`) that nobody can confirm are dead.
- **Proposal:** Adopt `tauri-specta` v2: derive `specta::Type` on every IPC struct/enum (incl. `AppError` from CMD-12 and `AppType`), generate `src/lib/bindings.ts` in `build.rs`/a `pnpm gen:bindings` step checked in CI, standardise on `rename_all = "camelCase"` for all commands and remove the `non_snake_case` allows, replace `serde_json::Value` returns with typed DTOs, and migrate `src/lib/api/*` to re-export the generated client.
- **Acceptance:** CI fails if `bindings.ts` is stale; `rg "allow\(non_snake_case\)" src-tauri/src/commands` is empty; `src/lib/api` contains no hand-written interface that duplicates a Rust type.

### CMD-31: Command/shell test coverage is thin and the test harness is awkward

**Type:** test · **Priority:** P1 · **Effort:** L · **Confidence:** high

- **Evidence:** Only six `*_test_hook` functions exist (`src-tauri/src/commands/proxy.rs:159-251`, `provider.rs:93-146`) and they are `pub` regardless of the `test-hooks` feature, which only toggles `doc(hidden)` (`src-tauri/Cargo.toml:18-20`); `src-tauri/src/lib.rs:44-45` `pub use commands::*` makes every command the crate's public API; integration tests (`src-tauri/tests/provider_commands.rs`, `mcp_commands.rs`, `proxy_commands.rs`) exercise `ProviderService`/`McpService`/hooks but no test references `create_tray_menu`, `handle_tray_menu_event`, `restore_proxy_state_on_startup`, `cleanup_before_exit`, `handle_deeplink_url` or `save_settings`; `lib.rs:2088-2134` tests only `classify_exit_request` and `enabled_proxy_apps_on_startup`; `tray.rs:1121-1532` tests only pure formatting helpers; tests share one global HOME and a global mutex (`src-tauri/tests/support.rs:7-24,64-68`) so they cannot run in parallel; no tests for CSP/capabilities, the settings race (CMD-06), raw-SQL pricing commands, or event payload shapes.
- **Problem:** The riskiest code (startup ordering, exit cleanup, tray, settings merge) has no safety net, and command behaviour is only testable by re-implementing commands as `_test_hook`s.
- **Proposal:** After CMD-10/CMD-12, test services directly and keep commands trivial; add `tauri::test::mock_app()`-based tests for `create_tray_menu`, `handle_tray_menu_event`, deep-link handling and the close/exit handlers; extract `startup::run(&AppState, StartupOptions)` from the `setup` closure so it is callable headlessly; use per-test temp HOME via `CC_SWITCH_TEST_HOME` instead of the shared dir + mutex; add a snapshot test of `generate_handler!` names vs. generated bindings (CMD-13); gate hooks behind `#[cfg(feature = "test-hooks")]` for real.
- **Acceptance:** `cargo test` runs the tests in parallel; coverage report shows `lib.rs` startup path, `tray.rs` event handling and `commands/settings.rs` exercised; `rg "_test_hook" src-tauri/src/commands` returns zero.

### CMD-05: `open_provider_terminal` writes the provider API key to a world-readable temp file

**Type:** security · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/commands/misc.rs:2719-2774` — `write_claude_config` serialises `env` (incl. `ANTHROPIC_AUTH_TOKEN` etc.) to `temp_dir()/claude_{providerId}_{pid}.json` via `std::fs::write` (default umask → typically 0644); cleanup relies on the spawned shell's `trap 'rm -f …' EXIT` (`misc.rs:2797`, `3128`) and only the Windows path removes it on failure (`3509-3511`); `providerId` is interpolated into the filename (`2725-2729`).
- **Problem:** On shared or multi-user machines the key is readable by other users for the lifetime of the terminal session, and if the terminal fails to launch or is killed the file is never removed.
- **Proposal:** Create the file with `OpenOptions::mode(0o600)` (Unix) / a per-user temp dir (Windows), prefer passing secrets via environment to the spawned process instead of a file, sanitise `providerId` to `[A-Za-z0-9_-]`, and sweep stale `claude_*_*.json` files at startup.
- **Acceptance:** `stat` shows 0600 on the temp config; a unit test rejects `providerId` containing path separators; no `claude_*.json` left in temp after a failed launch.

### CMD-11: Raw SQL and direct DAO calls inside commands bypass the service layer

**Type:** refactor · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/commands/usage.rs:125-171` (`get_model_pricing`: `lock_conn!`, `sqlite_master` probe, `SELECT … FROM model_pricing`), `175-248` (`INSERT OR REPLACE INTO model_pricing`), `262-274` (`DELETE FROM model_pricing`) — the only `lock_conn!` uses outside `database/`; direct `state.db.*` from commands with no service: `src-tauri/src/commands/settings.rs:629-720` (rectifier/optimizer/log config), `src-tauri/src/commands/config.rs:244-287`, `src-tauri/src/commands/failover.rs:12-59`, `src-tauri/src/commands/proxy.rs:92-149,312-321,413-442`, `src-tauri/src/commands/skill.rs:296-323`; `src-tauri/src/commands/openclaw.rs` and `hermes.rs` call the crate-root `openclaw_config`/`hermes_config` modules directly.
- **Problem:** Schema knowledge leaks into the IPC layer (the `sqlite_master` check at 133-144 exists because the command does not trust migrations), validation lives in two places, and the pattern invites more drift.
- **Proposal:** Add `database/dao/model_pricing.rs` with `list/upsert/delete`, a `services::pricing` module owning validation + backfill; route the remaining direct-DAO commands through the matching service (or a thin `SettingsService`); add a CI grep forbidding `lock_conn!` and `rusqlite::` in `commands/`.
- **Acceptance:** `rg "lock_conn!|rusqlite" src-tauri/src/commands` is empty; pricing commands have DAO + service tests.

### CMD-14: Event contract is untyped string literals and already drifts

**Type:** refactor · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** Backend emits 12 distinct event names as literals (`provider-switched` ×5, `usage-cache-updated`, `proxy-flags-changed`, `deeplink-import`, `deeplink-error`, `profile-applied`, `webdav-sync-status-updated`, `s3-sync-status-updated`, `proxy-official-warning`, `universal-provider-synced`, `update-download-progress`, `usage-log-recorded`); only `src-tauri/src/usage_events.rs:20` uses a const. `proxy-flags-changed` is emitted at `src-tauri/src/tray.rs:581,625` but has zero references in `src/`; `configLoadError` is listened to in `src/` but never emitted by the backend; `provider-switched` payloads differ by site — `{appType, providerId, source}` at `src-tauri/src/commands/failover.rs:166-170` vs `{appType, proxyEnabled, autoFailoverEnabled, providerId}` at `tray.rs:619-624` and `src-tauri/src/commands/profile.rs:76-81` (whose doc comment 58-62 warns the shape must match `App.tsx`).
- **Problem:** Dead emitters/listeners and shape divergence are invisible to the compiler; the frontend reducer must defensively handle optional fields.
- **Proposal:** Define `enum AppEvent { ProviderSwitched(ProviderSwitchedPayload), … }` with `specta::Type` + `tauri_specta::Event`, one `emit_event(&app, AppEvent)` helper, and generated typed `listen` helpers; delete `proxy-flags-changed` or add its listener; remove the `configLoadError` listener or emit it from `get_init_error`'s path.
- **Acceptance:** `rg "\.emit\(\s*\"" src-tauri/src` is empty; every event in the Rust enum has at least one generated-binding listener or is explicitly documented as tray-only.

### CMD-15: `show_in_tray` setting is exposed in the UI but never read by the backend

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/settings.rs:345-346,493-495,508` define `show_in_tray` (default true); `rg show_in_tray src-tauri/src` finds no reader; `src/types.ts:346`, `src/lib/schemas/settings.ts:12`, `src/hooks/useSettingsForm.ts:112,142,179` surface `showInTray` to users; `src-tauri/src/lib.rs:278-308` — the close handler hides the window whenever `minimize_to_tray_on_close` is true without checking tray visibility; the tray is always built at `lib.rs:911-956`.
- **Problem:** Turning the toggle off does nothing; if it were honoured naively, `minimize_to_tray_on_close=true` + no tray would make the app unreachable.
- **Proposal:** Either remove the setting from both sides, or implement it: `tray.set_visible(settings.show_in_tray)` on save, and force `minimize_to_tray_on_close=false` (or keep the macOS Dock icon) when the tray is hidden; add the invariant to `AppSettings::normalize_paths`.
- **Acceptance:** Toggling the setting shows/hides the tray icon at runtime; a unit test asserts the normalised settings never have `show_in_tray=false && minimize_to_tray_on_close=true`.

### CMD-16: Tray menu is fully rebuilt (≥6 DB queries + `block_on`) at 13 call sites, with four copies of the rebuild code

**Type:** performance · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/tray.rs:637-870` — `create_tray_menu` reads settings, loops `TRAY_SECTIONS` calling `get_all_providers` + `get_effective_current_provider` (which itself loads all providers again: `src-tauri/src/settings.rs:1003-1027`) + `block_on(get_live_backup)` per section, then `get_all_profiles`, and finally `tray.set_menu` which closes any open menu; the build-and-set sequence is duplicated at `tray.rs:568-572`, `612-616`, `908-920`, `src-tauri/src/lib.rs:193-211` and `src-tauri/src/commands/failover.rs:175-179`; `refresh_tray_menu`/`create_tray_menu` are called from 13 sites (lightweight toggle, profile apply, provider click, failover toggle, startup…); `update_tray_usage_labels` (`tray.rs:875-906`) and `refresh_all_usage_in_tray` (`1061-1090`) repeat the provider-table loads per section.
- **Problem:** Each provider switch or usage refresh costs O(apps × providers) DB work plus six `block_on`s; the in-place label update was added precisely because `set_menu` disrupts open menus, but most paths still do a full rebuild.
- **Proposal:** One `TrayController` owning menu handles: `rebuild()` (structure changes: providers/profiles added/removed, language), `update_current(app)` (check-mark + label only), `update_usage_labels()`; feed it from a `watch` channel of `AppState` changes instead of ad-hoc calls; replace `get_effective_current_provider`'s `get_all_providers` with `provider_exists(id)`; remove the `block_on` by caching proxy/takeover state (CMD-07).
- **Acceptance:** Switching a provider issues ≤ 2 SQL statements for the tray; `rg "create_tray_menu\(" src-tauri/src` shows one caller; open tray menu is not closed by a usage refresh (already partly handled) nor by a provider switch from the UI.

### CMD-18: Per-app enumeration is hand-rolled in ~15 places; adding an app type touches all of them

**Type:** refactor · **Priority:** P2 · **Effort:** L · **Confidence:** high

- **Evidence:** `src-tauri/src/app_config.rs:9-160` — `McpApps` and `SkillApps` are six-boolean structs with hand-written `is_enabled_for`/`set_enabled_for`/`enabled_apps`/`is_empty`; `src-tauri/src/settings.rs:27-80` — `VisibleApps` repeats the pattern; `settings.rs:411-450` — `*_config_dir` ×7 and `current_provider_*` ×8 fields, `567-637` normalises each by hand, `880-934` seven `get_*_override_dir` functions, `962-992` two 8-arm matches; `src-tauri/src/commands/config.rs:66-139,147-162,166-177` — three copies of the same per-app config-dir `match`; `src-tauri/src/tray.rs:156-185` — `TRAY_SECTIONS` covers only 4 of 8 apps; `src-tauri/src/lib.rs:1754` — `PROXY_STARTUP_APP_TYPES` another 4-list; `lib.rs:817-825` another explicit list.
- **Problem:** OpenCode/OpenClaw/Hermes/ClaudeDesktop silently lack tray switching and proxy-startup restore; every new app (the repo added Hermes and Grok Build recently) is an N-file change with high omission risk.
- **Proposal:** Introduce `AppSet` (bit-set keyed by `AppType`, serde as object) replacing the three bool structs; `AppType::config_dir()`, `::override_dir()`, `::supports_tray()`, `::supports_proxy()` methods or a `static APP_REGISTRY: [AppSpec; N]`; replace `current_provider_*`/`*_config_dir` with `HashMap<AppType, String>` (serde aliases keep the file format); derive tray sections and startup lists from the registry.
- **Acceptance:** Adding a dummy `AppType::Test` variant compiles after touching only `app_config.rs` + the registry; existing `settings.json` files still load (alias tests).

### CMD-19: Throwaway second `AppState`/`ProxyService` constructed inside commands

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/src/commands/import_export.rs:63-76` — `sync_current_providers_live` does `AppState::new(db)` inside `spawn_blocking`; `src-tauri/src/commands/sync_support.rs:10-15` — `run_post_import_sync` does the same (used by `import_config_from_file`, `webdav_sync_download`, `s3_sync_download`); `src-tauri/src/store.rs:12-22` — `AppState::new` constructs a fresh `ProxyService` (own `server`, `app_handle: None`, own circuit-breaker state); `src-tauri/src/lib.rs:965` — comment: "inject the same instance into global state to avoid inconsistency from duplicate creation".
- **Problem:** `ProviderService::sync_current_to_live` consults `state.proxy_service` (takeover detection, `is_running`) — the throwaway instance always reports "not running / not taken over", so a post-import sync while the proxy is active can overwrite takeover-owned live files.
- **Proposal:** Pass `Arc<AppState>` (or `State<'_, AppState>` cloned handles) into these helpers; make `AppState` fields `Arc`-cheap so `spawn_blocking(move || …)` can take a clone; add a debug assertion that only one `ProxyService` is ever created.
- **Acceptance:** `rg "AppState::new\(" src-tauri/src/commands` is empty; a test restores a backup while takeover is active and asserts the live file keeps proxy placeholders.

### CMD-20: Legacy / duplicate / stub commands remain registered

**Type:** tech-debt · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** 15 `copilot_*` commands (`src-tauri/src/commands/copilot.rs`) duplicate the 7 generic `auth_*` commands (`auth.rs:81-305`; `codex_oauth.rs:5-6` states auth is exposed via `auth_*`); six "兼容旧 API" skill commands (`skill.rs:181-291`, one of which re-fetches the whole remote catalogue to find one skill, 230-253); four deprecated MCP commands flagged `#[allow(deprecated)]` (`mcp.rs:50-154`); `import_from_deeplink` "legacy, kept for compatibility" (`deeplink.rs:25-42`); `get_circuit_breaker_stats` always returns `Ok(None)` (`proxy.rs:446-455`); `check_for_updates` merely opens a browser (`misc.rs:53-65`) beside `check_app_update_available`/`install_update_and_restart`; `migrate_app_config_dir_from_settings` is a no-op that logs on every boot (`src-tauri/src/app_store.rs:128-135`, `lib.rs:840`); `auth.rs` branches end in `unreachable!()` ×7.
- **Problem:** Surface area to secure/test/document grows; the frontend is the only consumer so compatibility shims have no external users.
- **Proposal:** Remove the listed commands after confirming no UI callsite (CMD-13 bindings make this mechanical); replace the `auth.rs` string dispatch with a `ManagedAuthProvider` trait object map; delete the no-op migration; fold `check_for_updates` into the updater flow.
- **Acceptance:** `generate_handler!` shrinks by ≥ 25 entries; no `#[allow(deprecated)]` in `commands/`; `unreachable!()` count in `auth.rs` is 0.

### CMD-22: `async` commands that block the runtime without `spawn_blocking`

**Type:** performance · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/commands/import_export.rs:82-122` — `save_file_dialog`/`open_file_dialog`/`open_zip_file_dialog` call `blocking_save_file`/`blocking_pick_file` inline in `async fn` (contrast `src-tauri/src/commands/config.rs:200-208` which correctly wraps `blocking_pick_folder` in `spawn_blocking`); `src-tauri/src/commands/workspace.rs:60-115,194-285` — directory scans and full-text search reading every memory file inline; `src-tauri/src/commands/mcp.rs` — all 14 commands are `async` but call sync `McpService`/`claude_mcp` file I/O; `src-tauri/src/commands/skill.rs:162-167` — `migrate_skill_storage` moves directories inline; `src-tauri/src/commands/settings.rs:62-127` — `save_settings` rewrites Codex live files inline; `src-tauri/src/commands/hermes.rs:110-114` — builds a new `reqwest::Client` per call; the DAO is `async fn` over a `std::sync::Mutex<Connection>` (`src-tauri/src/database/mod.rs:49,80`), so every "async" DB command also blocks its worker.
- **Problem:** A modal file dialog parks a tokio worker for minutes; large workspace searches stall other async commands and the proxy's own tasks that share the runtime.
- **Proposal:** Wrap blocking sections in `spawn_blocking` (helper `blocking_cmd!`), reuse the global HTTP client from `proxy::http_client`, and consider `tokio-rusqlite` or a dedicated DB thread with a channel so DAO `async fn`s are truly non-blocking.
- **Acceptance:** Clippy `clippy::unused_async` enabled; a CI grep flags `blocking_` calls not inside `spawn_blocking`; proxy latency is unaffected while a file dialog is open.

### CMD-23: Input-validation gaps on IPC parameters

**Type:** security · **Priority:** P2 · **Effort:** M · **Confidence:** medium

- **Evidence:** Raw `app_type: String` forwarded to the DAO without `AppType::from_str` at `src-tauri/src/commands/proxy.rs:54-63,119-127,312-321`, `src-tauri/src/commands/failover.rs:13-59`, `src-tauri/src/commands/usage.rs:252-258`, `src-tauri/src/commands/config.rs:279-287` (contrast `stream_check.rs:21` which takes `AppType` directly); `src-tauri/src/commands/env.rs:19-22` → `src-tauri/src/services/env_manager.rs:153-166` reads an arbitrary `backup_path` and rewrites shell profiles/registry from its contents; `src-tauri/src/commands/session_manager.rs:28-59` → `src-tauri/src/session_manager/terminal/mod.rs:3-30` executes an arbitrary `command` string in a terminal; `src-tauri/src/commands/balance.rs:4`, `coding_plan.rs:4-14`, `model_fetch.rs:12-18`, `global_proxy.rs:89-104` make backend HTTP requests to any webview-supplied URL (incl. `http://127.0.0.1`/link-local); `src-tauri/src/commands/skill.rs:327-336` extracts any `file_path`. Positive counter-examples: backups (`database/backup.rs:554,604,667`), sessions (`session_manager/mod.rs:151-161`), workspace filenames (`workspace.rs:10-44`).
- **Problem:** Each is user-driven today, but with the broad CSP (CMD-03) they let injected JS write to shell rc files, run commands, or use the backend as an SSRF proxy with the user's global proxy settings.
- **Proposal:** Take `AppType` (serde) instead of `String` everywhere; restrict `restore_env_backup` to files under `~/.cc-switch/env-backups/` and validate the schema; have `launch_session_terminal` accept a `{provider, session_id}` and build the resume command server-side; validate outbound URLs (scheme http(s), reject loopback/link-local unless the user explicitly configured them) in `services::model_fetch`/`balance`/`coding_plan`; whitelist `.zip/.skill` and dialog-returned paths for `install_skills_from_zip`.
- **Acceptance:** Unit tests for each rejection; `rg "app_type: String" src-tauri/src/commands` is empty.

### CMD-24: macOS deep links are handled twice (plugin listener + manual `RunEvent::Opened` arm)

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** low

- **Evidence:** `src-tauri/src/lib.rs:885-907` registers `app.deep_link().on_open_url(...)` which calls `handle_deeplink_url` (131-189) and emits `deeplink-import`; `lib.rs:1611-1667` separately matches `RunEvent::Opened { urls }` on macOS, re-parses and emits `deeplink-import` again with a copy of the same logic (and logs the raw URL, CMD-01); `tauri-plugin-deep-link` 2.4.7 (`src-tauri/Cargo.lock:5691-5692`) itself forwards `RunEvent::Opened` to `on_open_url` listeners; `src/components/DeepLinkImportDialog.tsx:53-62` calls `mergeDeeplinkConfig` on every event with no dedupe.
- **Problem:** If the plugin delivers the URL (which its v2 design does), each macOS open triggers two parses, two emits and two config-merge round-trips; at minimum the logic is duplicated in three places (single-instance args, `on_open_url`, `RunEvent::Opened`).
- **Proposal:** Verify with a log on both paths; keep only the plugin path (or only the `RunEvent` path on macOS, with a guard), and route all three entry points through `handle_deeplink_url` with a short-lived "last URL + timestamp" dedupe.
- **Acceptance:** Opening a `ccswitch://` link on macOS produces exactly one `deeplink-import` event (asserted via the plugin's event log in dev); `rg "parse_deeplink_url" src-tauri/src/lib.rs` shows one call.

### CMD-25: Multi-step command writes are non-transactional

**Type:** bug · **Priority:** P2 · **Effort:** M · **Confidence:** medium

- **Evidence:** `src-tauri/src/commands/config.rs:306-382` — `set_common_config_snippet` performs: legacy migration → `set_config_snippet` → `set_config_snippet_cleared` → `sync_current_provider_for_app` (live file write) → OMO file write, each `?`-propagating with no rollback; `src-tauri/src/commands/settings.rs:62-127` — `save_settings` persists settings then re-writes Codex live, with a hand-rolled rollback only for that one field; `src-tauri/src/commands/proxy.rs:133-149` — DB update then in-memory circuit-breaker update; `src-tauri/src/commands/failover.rs:139-162` — switch target, then write flag (partial rollback at 147-149 only for the auto-added queue item).
- **Problem:** A failure mid-sequence leaves DB flags and live files disagreeing (e.g. snippet saved but current provider's live file not regenerated), which the startup "auto-extract snippet" logic (`lib.rs:1806-1854`) can then compound.
- **Proposal:** Move each sequence into a service method that opens a SQLite transaction for the DB steps and orders file writes last (write-ahead: compute all file contents, then commit DB, then write files, with a best-effort revert on file failure); return a structured `warnings` list like `SwitchResult` does.
- **Acceptance:** Fault-injection tests (fail the live write) leave DB state unchanged.

### CMD-29: Crash handling and logging retention lose the context you need after a crash

**Type:** feature · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/panic_hook.rs:80-185` writes `crash.log` with backtrace but forces `RUST_BACKTRACE=1` process-wide (82-84); `src-tauri/src/lib.rs:349-351` deletes the previous `cc-switch.log` on every start, so after a crash-and-relaunch the pre-crash log is gone; `lib.rs:365-371` relies on `KeepSome(2)` to work around a plugin underflow and caps a single file at 1 GB; log level is `Trace` until the DB config loads (356, 968-979); no opt-in crash reporting (no `sentry`/`tracing` in `src-tauri/Cargo.toml`); `tokio::task` panics inside spawned startup tasks (`lib.rs:1042-1151`) are only visible if the hook fires.
- **Problem:** Support cases ("app crashed on upgrade") cannot be diagnosed from a fresh log; `Trace` during startup is where the secret-leak in CMD-01 lands.
- **Proposal:** Rotate to `cc-switch.log.1..3` instead of deleting; read the persisted level before initialising the logger (store it in the Tauri Store alongside `app_config_dir`, since the DB is not open yet); adopt `tracing` + `tracing-subscriber` (JSON file layer + stdout) with a `log` bridge; add an opt-in crash/diagnostics bundle exporter command (zips logs + crash.log + sanitized settings) and, optionally, an opt-in Sentry DSN.
- **Acceptance:** After a forced panic and relaunch, both `crash.log` and the previous run's log are present; startup logs contain no `TRACE` lines when the saved level is `info`.

### CMD-30: No command-level tracing, timing or request ids

**Type:** feature · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** 285 commands with ad-hoc `log::info!` at entry in a few (`src-tauri/src/commands/usage.rs:126,169`, `deeplink.rs:11,21,31`, `failover.rs:85-87`) and none in most; no wrapper around `generate_handler!` (`src-tauri/src/lib.rs:1201-1525`); errors are stringified before logging so the original `AppError` variant is lost (`error.rs:108-112`).
- **Problem:** Slow or failing IPC calls (CMD-08/09 symptoms) cannot be attributed; the usage dashboard cannot show "backend took 3 s for get_usage_trends".
- **Proposal:** A `#[cc_command]` proc-macro (or a thin `instrumented(name, async fn)` helper) that records duration, result variant and a request id into `tracing` spans, emits a `command-slow` event over 500 ms in debug builds, and tags logs with the command name; pair with CMD-29.
- **Acceptance:** Every command appears in logs as `cmd=get_providers dur_ms=12 ok=true`; a `cmd-stats` debug page can list p95 per command.

### CMD-32: No developer documentation for the IPC surface, events, startup sequence or security posture

**Type:** docs · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `docs/` contains `guides`, `images`, `release-notes`, `user-manual`; a grep of `docs/`, `README.md`, `CLAUDE.md`, `AGENTS.md` for `tauri::command`, `invoke(` or `IPC` matches only release notes; the only description of the startup order is inline comments in `src-tauri/src/lib.rs:318-1200`; the reasoning for `KeepSome(2)` (365-367), the restart deadlock (1542-1557) and the sync-command constraint (`commands/profile.rs:161-164`) exist only as code comments.
- **Problem:** Contributors cannot discover which of the 281 commands to call, what events exist, or why sync/async rules exist, which is how the duplicated workflows in CMD-10 arose.
- **Proposal:** `docs/dev/architecture.md` (layers, startup sequence diagram, exit/restart paths), `docs/dev/ipc.md` generated from the specta bindings (commands, params, errors, events), `docs/dev/security.md` (CSP, capabilities, secret handling, path validation rules), and a `CONTRIBUTING` rule list ("no `block_on` outside …", "commands must be async", "no business logic in commands").
- **Acceptance:** Docs exist and are linked from README; the IPC doc is regenerated in CI and fails when stale.

### CMD-33: Feature — headless / CLI mode reusing the service layer

**Type:** feature · **Priority:** P2 · **Effort:** L · **Confidence:** medium

- **Evidence:** `src-tauri/src/store.rs:12-22` — `AppState` needs only a `Database`; `src-tauri/tests/support.rs:72-75` already builds it without Tauri; services take `&AppState` (`ProviderService::switch`, `ProxyService::start`); blockers are the `AppHandle` couplings (`services/proxy.rs:62`, `tray` calls from commands `commands/profile.rs:92`, `failover.rs:175`) and logic stranded in commands (CMD-10).
- **Problem:** Users running on servers/CI (the README targets developers juggling multiple providers) have no way to `cc-switch switch codex <id>` or start the proxy without the GUI; the same gap blocks scripted E2E tests of the startup path.
- **Proposal:** A `cc-switch-cli` binary (clap) in the same workspace: `providers list/switch/import`, `proxy start/stop/status`, `profile apply`, `export/import`, `doctor` (runs the startup checks headlessly); introduce a `UiNotifier` trait (tray refresh, events) with a no-op impl for CLI so services never touch `AppHandle`.
- **Acceptance:** `cargo run -p cc-switch-cli -- providers switch claude foo` modifies the live file identically to the GUI path (shared service test).

### CMD-36: Exit/restart paths rely on `std::process::exit` after fixed sleeps

**Type:** tech-debt · **Priority:** P2 · **Effort:** M · **Confidence:** medium

- **Evidence:** `src-tauri/src/lib.rs:1567-1586` — user exit: spawn async cleanup, `sleep(100ms)`, then `std::process::exit(0)`; `src-tauri/src/commands/settings.rs:173-190` — `restart_app` sleeps 100 ms then `app.restart()` from a spawned task; `settings.rs:234-250` — Windows updater path: cleanup, `sleep(100ms)`, `install` (which itself exits); `lib.rs:2082-2086` — `restart_process` bypasses plugin exit hooks by design; the restart-path deadlock is documented at 1542-1557; `cleanup_before_exit` (1689-1724) awaits proxy stop/restore with no timeout.
- **Problem:** Fixed sleeps are neither sufficient (large restore + SQLite WAL checkpoint) nor minimal; a hung proxy stop blocks exit forever; three slightly different shutdown sequences exist.
- **Proposal:** A `shutdown::run(reason) -> impl Future` used by all three paths: stop proxy with a bounded timeout, restore live files, `PRAGMA wal_checkpoint(TRUNCATE)`, flush loggers, save window state, remove tray, then exit/restart; log each stage's duration.
- **Acceptance:** One shutdown function; exit completes < 1 s with the proxy running and never hangs when the proxy task is stuck (timeout path tested).

### CMD-37: Window recreated by lightweight mode loses Linux/decoration/theme setup

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/src/lib.rs:1153-1166` — `setup` disables WebKitGTK hardware acceleration on the initial `main` webview; `lib.rs:1173-1174` — `set_decorations(!settings.use_app_window_controls)` applied once; `src-tauri/src/commands/misc.rs:3524-3535` — theme is set per-window on demand; `src-tauri/src/lightweight.rs:19-24` destroys `main`, and `58-69` rebuilds it from `tauri.conf.json` via `WebviewWindowBuilder::from_config` without re-applying any of the above (only focus/skip-taskbar/dock policy at 71-90).
- **Problem:** After leaving lightweight mode on Linux the new webview runs with hardware acceleration enabled (the white-screen condition the setup code guards against) and with system decorations even when app window controls are configured; the webview reloads the SPA from scratch (losing in-memory state).
- **Proposal:** Extract `window_setup::apply(&window, &settings)` (HW-accel policy, decorations, theme, Linux nudge) and call it from both `setup` and `exit_lightweight_mode`; or implement lightweight mode as `hide()` + webview suspension rather than destroy.
- **Acceptance:** A Linux smoke test toggling lightweight mode twice shows no white screen and preserves the decoration setting; the function is unit-covered with `tauri::test::mock_app`.

### CMD-17: Two locale detectors and a second hard-coded i18n table in Rust

**Type:** tech-debt · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/tray.rs:96-141` — `TrayTexts::from_language` hard-codes en/ja/zh-TW/zh strings; `tray.rs:67-94` — `map_locale_to_tray_language` via `sys_locale`; `src-tauri/src/lib.rs:1890-1896` — a different `is_chinese_locale()` reading only `LANG`/`LC_ALL`/`LC_MESSAGES` (wrong on Windows/macOS where those are unset); `lib.rs:1900-2012` — the migration/DB dialogs support only zh/en; tray.rs:791-795 keeps scope labels "not in i18n".
- **Problem:** Adding a language (the UI already has four) means editing Rust tables; macOS/Windows users with Chinese system locale get English startup dialogs while the tray is Chinese.
- **Proposal:** Ship a tiny `i18n.json` resource (generated from the frontend locale files for the ~20 backend strings) loaded once, with a single `backend_language()` that uses `settings.language` → `sys_locale` fallback; use it for tray, dialogs and `Localized` errors.
- **Acceptance:** One locale-detection function; adding a locale string requires no Rust edit; dialogs appear in Japanese on a ja-JP macOS.

### CMD-21: Commands load the whole provider table to find one row; settings cloned per call

**Type:** performance · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/commands/provider.rs:470-474` (`query_provider_usage_inner`), `src-tauri/src/commands/misc.rs:2612-2617` (`open_provider_terminal`), `src-tauri/src/commands/proxy.rs:388-392`, `src-tauri/src/commands/stream_check.rs:26-29`, `src-tauri/src/commands/omo.rs:23-34,59-70`, `src-tauri/src/settings.rs:1010` — all call `get_all_providers`/`ProviderService::list` then `.get(id)`, while `get_provider_by_id` exists and is used at `tray.rs:1083`; `get_settings()` clones the ~45-field `AppSettings` at 21 sites including the window close handler (`lib.rs:290`) and every tray rebuild (`tray.rs:641`).
- **Problem:** O(n) deserialisation of every provider JSON blob per usage poll (the tray polls up to 4 per hover) and per terminal launch.
- **Proposal:** Use `get_provider_by_id`; add `Database::provider_exists`; expose cheap accessors (`settings::with(|s| …)`) instead of cloning.
- **Acceptance:** `rg "get_all_providers\(.*\)\s*.*\n.*\.get\(" -U src-tauri/src/commands` is empty; tray hover issues one `SELECT … WHERE id=?` per section.

### CMD-26: `stream_check_all_providers` probes sequentially

**Type:** performance · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/commands/stream_check.rs:73-109` — `for (id, provider) in providers { … check_with_retry(...).await … }`; each check has timeout × retries from `StreamCheckConfig`.
- **Problem:** With 20 providers and a 10 s timeout the UI waits minutes; this also belongs in a service (CMD-10).
- **Proposal:** `futures::stream::iter(...).buffer_unordered(4)` with per-provider results emitted as `stream-check-progress` events; keep DB logging per result.
- **Acceptance:** Wall time for N providers ≈ max(single) × ceil(N/4).

### CMD-27: `webdav_sync.rs` and `s3_sync.rs` are copy-paste twins returning untyped JSON

**Type:** refactor · **Priority:** P3 · **Effort:** M · **Confidence:** high

- **Evidence:** A `sed`-normalised diff of `src-tauri/src/commands/webdav_sync.rs` vs `s3_sync.rs` differs only in field names (`password` vs `secret_access_key`) and formatting; both return `json!({"success": true, …})` (`webdav_sync.rs:98-101,159,168`; `src-tauri/src/commands/sync_support.rs:36-54`); the auto-sync workers are likewise parallel (`src-tauri/src/lib.rs:957-964`).
- **Problem:** Every sync fix must be applied twice; the frontend parses ad-hoc `success/message/warning/empty` objects.
- **Proposal:** A `SyncBackend` trait (`check_connection`, `upload`, `download`, `fetch_remote_info`, `settings()`), one generic command set taking `backend: SyncBackendKind`, typed `SyncResult { backup_id, warning }` DTOs.
- **Acceptance:** One command file for cloud sync; adding a third backend (e.g. GitHub Gist) touches only a new `impl SyncBackend`.

### CMD-28: Two ad-hoc thread-sleeping debouncers and two "global AppHandle" patterns

**Type:** tech-debt · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/usage_events.rs:46-68` — `std::thread::spawn` + `sleep(200ms)` per burst; `src-tauri/src/tray.rs:1006-1022` — `spawn_blocking` + `std::thread::sleep(50ms)`; `usage_events.rs:25` stores the handle in a `OnceLock<AppHandle>` while `src-tauri/src/services/proxy.rs:62,504-508` stores it in `Arc<RwLock<Option<AppHandle>>>` set via `block_on` (`lib.rs:496`).
- **Problem:** Blocking-pool threads are consumed for timers; two conventions for the same need; `usage_events` is silently a no-op in tests (48-50) so emission is untested.
- **Proposal:** A single `events::Emitter` managed in `AppState` (holds `AppHandle`, offers `emit(AppEvent)` and `emit_debounced(AppEvent, Duration)` built on `tokio::time::sleep`); use it from DAO hooks, tray and commands.
- **Acceptance:** `rg "std::thread::sleep|thread::spawn" src-tauri/src/{tray,usage_events}.rs` is empty; emitter has a unit test with a fake sink.

### CMD-34: Feature — global hotkeys and a second (quick-switcher) window

**Type:** feature · **Priority:** P3 · **Effort:** M · **Confidence:** medium

- **Evidence:** Window control is single-window: `src-tauri/capabilities/default.json:5-7` (`"windows": ["main"]`), `src-tauri/tauri.conf.json:13-27`; lightweight mode destroys and recreates `main` (`src-tauri/src/lightweight.rs:7-31,58-69`); the tray is the only non-window entry point (`tray.rs:942-997`); no `tauri-plugin-global-shortcut` in `Cargo.toml`.
- **Problem:** Switching providers currently needs the full window or a multi-level tray menu; power users ask for keyboard access and a compact palette.
- **Proposal:** Add `tauri-plugin-global-shortcut` with a configurable "toggle window" and "quick switch" shortcut; add a small `switcher` window (separate capability with a minimal permission set) that lists providers for the focused app; make lightweight mode hide rather than destroy (see CMD-37 for why recreation is risky).
- **Acceptance:** Shortcut opens the palette on all three OSes; capabilities file grants the palette only `core:window:allow-close/hide`.

### CMD-35: Feature — extension API for provider adapters and usage scripts

**Type:** feature · **Priority:** P3 · **Effort:** XL · **Confidence:** low

- **Evidence:** JS usage scripts are already a user-extension point run from the backend (`src-tauri/src/commands/provider.rs:674-704` `testUsageScript`, `usage_script.rs`), and template types are dispatched by string (`provider.rs:15-19,478-669`); each new provider "kind" (Copilot, Codex OAuth, coding plans, balance) added hand-written branches in commands and tray (`tray.rs:14-40` tier label tables).
- **Problem:** Every new vendor requires a release; the dispatch tables in commands/tray are the integration point today.
- **Proposal:** Define a `ProviderKind` manifest (JSON in `~/.cc-switch/extensions/<id>/`: display, auth mode, usage endpoint template or script, tray summary mapping), loaded by a `services::extensions` registry; migrate the built-in kinds to manifests first; expose `list_extensions`/`reload_extensions` commands.
- **Acceptance:** Adding a new coding-plan vendor is a manifest drop-in with no Rust change; built-in kinds pass existing usage tests.

### CMD-38: `set_app_config_dir_override` accepts unvalidated paths and silently falls back later

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/app_store.rs:74-106` — stores any non-empty trimmed string; `app_store.rs:47-53` — on the next start a missing path logs a warning and silently uses the default dir; the command at `src-tauri/src/commands/settings.rs:294-302` returns `Ok(true)` without checking existence/writability; `src-tauri/src/panic_hook.rs:17-19` captures the dir once per process, so a changed override only takes effect after `restart_app`.
- **Problem:** A typo or an unmounted external drive makes the app start with an empty database and no UI indication of why the user's providers vanished.
- **Proposal:** Validate (absolute, exists or creatable, writable, not inside the old dir) in the command and return structured errors; on fallback at startup, set an `init_status` warning surfaced by `get_init_error` so the UI explains it; copy/offer to migrate the existing DB when changing the dir.
- **Acceptance:** Setting a non-existent dir returns an error; starting with an unreachable override shows a banner instead of an empty provider list.

### CMD-39: Updater has three overlapping entry points and the frontend calls the plugin directly

**Type:** tech-debt · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/commands/misc.rs:53-65` (`check_for_updates` opens the GitHub page), `src-tauri/src/commands/settings.rs:269-285` (`check_app_update_available`, no UI callsite), `settings.rs:192-267` (`install_update_and_restart`), and `src/lib/updater.ts:31` importing `@tauri-apps/plugin-updater` `check` directly (requiring `updater:default` in the capability).
- **Problem:** Update logic is split across Rust and TS, the capability grant is wider than needed (CMD-03), and the GitHub-page fallback is dead weight.
- **Proposal:** One `UpdateService` in Rust (`check`, `download_and_install` with progress events), the frontend uses only the typed commands; drop the plugin grant.
- **Acceptance:** `rg "plugin-updater" src/` is empty; `updater:default` removed from capabilities; update flow still works on all OSes.
