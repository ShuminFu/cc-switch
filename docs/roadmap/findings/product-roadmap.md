# Product strategy & feature roadmap — findings (PRODUCT-xx)

Part of the [next-phase proposals](../README.md). 65 findings. Priorities: P1 = 6, P2 = 41, P3 = 18.

## Area summary

CC Switch (v3.17.0, MIT, Tauri 2 + React 18, 100k+ GitHub stars per `docs/release-notes/v3.16.3-en.md`) has evolved from a "provider switcher" into an all-in-one control plane for eight AI coding tools: 267 Tauri commands across 34 command modules, a 30-module Rust proxy with four protocol bridges (Anthropic/Chat/Responses/Gemini), unified MCP/Prompts/Skills sync, usage/cost accounting from both proxy logs and CLI session logs, cloud sync (WebDAV/S3), OAuth centers (Copilot, Codex), managed CLI-tool lifecycle, and (since 3.17.0) "Projects" snapshots. Release velocity is very high: eight releases between 2026-05-16 (v3.15.0, 127 commits) and 2026-07-13 (v3.17.0, 69 commits), with ~40-50% of each release's entries being *fixes* concentrated in four hotspots: proxy format-conversion bridges (Codex Chat/Responses/Anthropic), usage accounting (cache tokens, subagents, real upstream model), Codex `config.toml`/MCP projection (file blanking, deleted servers returning), and quota/usage-script queries (Zhipu tiers, retries). That fix distribution is the clearest signal of user pain: the proxy and billing paths are where power users live and where regressions hurt. The biggest unmet needs per persona are: (a) solo relay users have no budget alerts (the `limitDailyUsd` meta exists in the backend but is never surfaced or enforced), no key rotation, and no system notifications; (b) proxy power users have no proactive health probing (the manual documents it, `proxy/health.rs` is a 7-line stub), no model-aware/smart routing, no headless proxy, no CLI, and takeover/tray/profiles only cover a subset of the eight apps; (c) team leads can share via deep links and WebDAV/S3 but with no encryption, policy, audit log, or signed remote catalog; (d) security-conscious users find API keys in plaintext in SQLite, SQL exports and cloud-sync bundles, a LAN-bindable proxy with no listener auth, a permissive CSP, and three ToS-sensitive reverse-proxy features; (e) the Chinese relay ecosystem (35 of ~70 Claude presets are paid partners, hardcoded in `src/config/*Presets.ts`) churns every release (LionCC removed, MiniMax demoted, SudoCode promoted/demoted/promoted), requiring app releases for commercial changes and offering no in-app affiliate disclosure; (f) contributors face a 112-file / 4.5k-line change to add one app (Grok Build commit `1c0ee0c`), a 5,400-line `commands/misc.rs`, no adapter trait, no e2e tests, and stale CONTRIBUTING paths. Recommended next-phase themes: (1) "Proxy as a product" — proactive health, notifications, smart routing, headless daemon + CLI; (2) "Trust & governance" — OS keychain vault, encrypted sync, listener auth, signed remote preset/sponsor catalog with disclosure, ToS gating; (3) "Projects everywhere" — per-directory auto-binding, all eight apps in profiles/tray/universal/takeover, usage attribution by project; (4) "Platform SDK" — adapter trait + plugin surface so the next wave of tools (Qwen Code, Kimi CLI, Goose, Aider, Zed) does not cost 4.5k lines each; (5) "Reliability engineering" — bridge conformance fixtures and e2e switch/takeover tests to stop the fix treadmill.

## Capability inventory

Apps (`src/lib/api/types.ts:2-10`, `src/config/appConfig.tsx:18-27`): claude, claude-desktop, codex, gemini, grokbuild, opencode, openclaw, hermes. Per-app adapters: `src-tauri/src/{codex(3497),claude_desktop(2216),hermes(2444),app(1250),openclaw(1089),gemini(651),grok(516),opencode(267)}_config.rs`.
Navigation views (`src/App.tsx` `View` union): providers, settings, prompts, skills, skillsDiscovery, mcp, agents(stub), universal, sessions, workspace, openclawEnv, openclawTools, openclawAgents, hermesMemory. Settings tabs (`SettingsPage.tsx:228-512`): general, proxy, auth, advanced{directory,data,backup,cloudSync,connectivityCheck,logConfig}, usage, about(tool management).
Backend commands (267 `#[tauri::command]`, `src-tauri/src/commands/*.rs`), grouped:
- Providers (29): get/add/update/delete/switch, remove_from_live_config, import_default_config, Claude Desktop status/routes/import/official, ensure_codex_official_provider, queryProviderUsage, testUsageScript, read_live_provider_settings, test_api_endpoints, custom endpoints CRUD, sort order, universal providers CRUD+sync, OpenCode/OpenClaw/Hermes live import & ids.
- Proxy (24) + failover (6) + stream_check (4) + global_proxy (5): start/stop(+restore), takeover per app, status, per-app config, cost multiplier, pricing source, switch_proxy_provider, provider_health, circuit breaker get/update/reset/stats; failover queue add/remove/get, auto-failover toggle; stream_check provider/all + config; upstream HTTP proxy url/test/scan.
- Usage (13) + balance/coding_plan/subscription (3): summary (by app), trends, provider/model stats, request logs/detail, model pricing CRUD, check_provider_limits (unused by UI), sync_session_usage, data sources; get_balance, get_coding_plan_quota, get_subscription_quota.
- MCP (14): unified CRUD/toggle per app, import_mcp_from_apps, validate_mcp_command, legacy Claude MCP ops. Prompts (6): CRUD/enable/import/current-file. Skills (24): installed/discover/install/uninstall(+unified, per-app)/update/check_updates/backups/restore/repos CRUD/skills.sh search/zip install/unmanaged scan/import/migrate storage. Profiles (6): list/create/update/delete/clear/apply.
- Settings (19) + config (14) + import_export (11) + plugin (6) + misc (12): settings get/save, auto launch, rectifier/optimizer/copilot-optimizer/log config, app config dir override, Codex unified-history restore, update check/install/restart; config dirs/paths/open folder, common-config snippets (Claude JSON / Codex TOML / extract); export/import SQL, sync_current_providers_live, DB backups create/list/restore/rename/delete, file dialogs; Claude plugin config & onboarding skip; open_external, clipboard, portable mode, migration results, tool versions/probe/lifecycle (install/update), open_provider_terminal, window theme.
- Auth: copilot (8: device flow, accounts, token, models, usage) and Codex OAuth (`auth_*` via auth.rs/codex_oauth.rs: start_login/poll/list/remove/set_default/logout/status). OpenClaw (14): live provider, health scan, default model, model catalog, agents defaults, env, tools. Hermes (10): live providers, model config, memory get/set/limits/enable, web UI/dashboard. Workspace (8): OpenClaw workspace files + daily memory CRUD/search. Sessions (5): list/messages/launch terminal/delete(+batch). Deeplink (4): parse/merge/import. Env (3): check conflicts/delete/restore backup. Cloud sync: webdav (5) + s3 (5). Lightweight mode (3). OMO (6).
Proxy routes (`src-tauri/src/proxy/server.rs:292-366`): /health, /status, /v1/messages (+/claude), /chat/completions, /models, /v1/models, /responses (+aliases), Gemini /v1beta/*; no admin/dashboard API; gateway token only on the Claude Desktop route (`handlers.rs:260`).
Coverage matrix (code-verified): proxy takeover = Claude/Codex/Gemini/GrokBuild (+Claude Desktop route) (`services/proxy.rs`, `App.tsx` toolbar guard); tray submenus = Claude/Codex/Gemini/GrokBuild (`tray.rs`); profiles scope = Claude/Claude Desktop/Codex (`services/profile.rs:42-46`); universal providers = Claude/Codex/Gemini (`src/types.ts:537-541`); prompts = Claude/Claude Desktop/Codex/Gemini/GrokBuild/OpenCode (`prompt_files.rs:12`); skills/MCP = all except OpenClaw (`appConfig.tsx:30-37`), Claude Desktop excluded from MCP sync (manual 3.1); stream check = Claude/Claude Desktop/Codex/OpenCode/OpenClaw/Hermes (`services/stream_check.rs`); sessions = 7 sources; "open terminal with provider" = Claude only (`App.tsx` `onOpenTerminal={activeApp === "claude" ...}`).
Presets (`src/config/*Presets.ts`, 11.2k lines): Claude 71 entries / 35 `isPartner`, Claude Desktop 68/35, Codex 64/34, Gemini 22/14, Hermes 225/33, OpenClaw 152/33, OpenCode 231/29; `primePartner` heart badge (Kimi) on 10 entries; categories official/cn_official/cloud_provider/aggregator/third_party/custom/omo. MCP presets: 5 (`mcpPresets.ts`). Universal presets: NewAPI, custom gateway. Usage-script templates: custom, generic, newapi, token_plan, official_subscription, volcengine, zenmux, zhipu_team + 5 balance presets (`UsageScriptModal.tsx`).
Platform: Tauri plugins log/opener/process/updater/dialog/store/deep-link/window-state/single-instance (`Cargo.toml:30-38,87`); no notification, global-shortcut, keyring, or CLI (clap) crates; i18n zh/zh-TW/en/ja (`src/i18n/index.ts:9`); DB schema v15, 16 tables; tests: 68 frontend test files + 12 Rust integration test files, no e2e; CI = typecheck/format/vitest + fmt/clippy/cargo test; release = macOS signed+notarized, Tauri-updater minisign for all, Windows x64/ARM64, Linux x64/ARM64 deb/rpm/AppImage, Flatpak manual only.

## Index

| ID | Title | Type | Priority | Effort | Confidence | Verdict |
|---|---|---|---|---|---|---|
| [PRODUCT-01](#product-01) | Secrets vault: OS keychain for API keys/tokens and encrypted export & cloud-sync bundles | risk | P1 (first pass P0) | L | high | partially-confirmed |
| [PRODUCT-02](#product-02) | Authenticate the proxy listener when bound to a non-loopback address | risk | P1 (first pass P0) | S | high | confirmed |
| [PRODUCT-04](#product-04) | Proxy bridge conformance suite (golden fixtures per protocol pair) | refactor | P1 (first pass P0) | M | medium | partially-confirmed |
| [PRODUCT-62](#product-62) | THEME — "Trust & governance" (vault, listener auth, signed catalog, disclosure, policy, audit, signing) | risk | P1 (first pass P0) | XL | medium | partially-confirmed |
| [PRODUCT-63](#product-63) | THEME — "Projects everywhere" (per-directory binding, all apps in profiles/tray/universal/takeover, usage by project) | feature | P1 | XL | high | partially-confirmed |
| [PRODUCT-65](#product-65) | THEME — "Reliability engineering" (bridge fixtures, e2e scenarios, beta ring, diagnostics) | refactor | P1 | L | high | partially-confirmed |
| [PRODUCT-03](#product-03) | Proactive provider health probing (documented but unimplemented) | bug | P2 (first pass P0) | M | high | confirmed |
| [PRODUCT-05](#product-05) | Finish budget limits & alerts (backend exists, UI/enforcement missing) | feature | P2 (first pass P1) | M | high | confirmed |
| [PRODUCT-06](#product-06) | System/tray notifications for failover, circuit open, budget, quota exhaustion and sync errors | feature | P2 (first pass P1) | M | high | confirmed |
| [PRODUCT-07](#product-07) | Remote, signed preset & sponsor catalog (decouple commercial changes from app releases) | feature | P2 (first pass P1) | L | medium | partially-confirmed |
| [PRODUCT-08](#product-08) | In-app affiliate/sponsor disclosure and preset provenance | risk | P2 (first pass P1) | S | high | partially-confirmed |
| [PRODUCT-09](#product-09) | ToS-sensitive feature gate and README wording fix ("signature bypass") | risk | P2 (first pass P1) | S | medium | partially-confirmed |
| [PRODUCT-10](#product-10) | Headless proxy daemon (proxy survives app quit / runs without window) | feature | P2 (first pass P1) | L | medium | partially-confirmed |
| [PRODUCT-11](#product-11) | CLI companion (`ccs`) over the same SQLite store | feature | P2 (first pass P1) | L | high | partially-confirmed |
| [PRODUCT-12](#product-12) | Per-project (cwd / git repo) automatic profile binding | feature | P2 (first pass P1) | L | medium | partially-confirmed |
| [PRODUCT-13](#product-13) | Model-aware "smart routing" (cheapest/fastest/healthiest provider per model, model fallbacks) | feature | P2 | L | high |  |
| [PRODUCT-14](#product-14) | Usage attribution by project/session and per-session cost in the Session Manager | feature | P2 | M | high |  |
| [PRODUCT-15](#product-15) | Tray parity for all eight apps plus proxy/failover controls | ux | P2 | M | high |  |
| [PRODUCT-16](#product-16) | Universal providers for OpenCode/OpenClaw/Hermes/Grok Build | feature | P2 | M | high |  |
| [PRODUCT-17](#product-17) | Extend Projects (profiles) scope to Gemini/Grok Build/OpenCode/OpenClaw/Hermes | feature | P2 | M | high |  |
| [PRODUCT-18](#product-18) | Proxy takeover (routing/failover/usage) for OpenCode, OpenClaw and Hermes | feature | P2 | L | medium |  |
| [PRODUCT-19](#product-19) | Command palette and global hotkeys | ux | P2 | M | high |  |
| [PRODUCT-20](#product-20) | Deep links for navigation and actions (`ccswitch://open/...`, `ccswitch://switch?...`) | feature | P2 | S | high |  |
| [PRODUCT-21](#product-21) | Config time machine: per-entity change history with diff and rollback | feature | P2 | M | high |  |
| [PRODUCT-22](#product-22) | Share-safe export and import preview (redaction, selective scope, dry-run) | feature | P2 | S | high |  |
| [PRODUCT-23](#product-23) | MCP server health check, tool listing and richer catalog | feature | P2 | M | high |  |
| [PRODUCT-24](#product-24) | Team policy ("managed mode") file for shared machines and orgs | feature | P2 | L | medium |  |
| [PRODUCT-25](#product-25) | Local audit log of configuration mutations and proxy admin actions | feature | P2 | M | high |  |
| [PRODUCT-26](#product-26) | Import from adjacent tools' configs (claude-code-router, one-api/new-api, Cline/Roo, Continue, Aider) | feature | P2 | M | medium |  |
| [PRODUCT-27](#product-27) | First-run onboarding wizard (detect tools, pick provider, choose routing mode) | ux | P2 | M | high |  |
| [PRODUCT-28](#product-28) | Accessibility pass (keyboard navigation, ARIA, reduced motion, contrast) | ux | P2 | M | high |  |
| [PRODUCT-29](#product-29) | Diagnostics bundle ("Report a problem") with redacted logs, settings and tool versions | feature | P2 | S | high |  |
| [PRODUCT-31](#product-31) | Request-log retention and privacy controls | feature | P2 | S | medium |  |
| [PRODUCT-32](#product-32) | API key pool / rotation per provider (multiple keys, rotate on 429/401) | feature | P2 | M | high |  |
| [PRODUCT-33](#product-33) | Capability matrix in docs and in-app ("what works for this app") | docs | P2 | S | high |  |
| [PRODUCT-34](#product-34) | Documentation bugs batch (CONTRIBUTING paths/scripts, issue-template apps, port, health claim, zh-only guide) | docs | P2 | S | high |  |
| [PRODUCT-35](#product-35) | Windows Authenticode signing (SmartScreen) and SBOM/provenance for releases | risk | P2 (first pass P1) | S | high | confirmed |
| [PRODUCT-36](#product-36) | Deep-link `usageScript` hardening (script preview, explicit opt-in, SSRF review) | risk | P2 | S | medium |  |
| [PRODUCT-40](#product-40) | Pricing currency semantics and FX setting | bug | P2 | S | medium |  |
| [PRODUCT-43](#product-43) | Adjacent tool support wave 1 (Qwen Code, Kimi CLI, Goose, Aider, Zed) via adapter trait | feature | P2 | XL | medium |  |
| [PRODUCT-44](#product-44) | Backend modularization: split `commands/misc.rs`, introduce `cc-switch-core` crate | refactor | P2 | L | high |  |
| [PRODUCT-45](#product-45) | End-to-end tests for switch/takeover/failover flows | refactor | P2 | M | high |  |
| [PRODUCT-51](#product-51) | Stream check coverage gap for Gemini and Grok Build | bug | P2 | S | medium |  |
| [PRODUCT-54](#product-54) | Sponsor preset governance policy (criteria, review cadence, removal, disclosure) as a public document | risk | P2 (first pass P1) | S | high | confirmed |
| [PRODUCT-55](#product-55) | Information architecture for eight apps (sidebar + app-scoped feature rail, progressive disclosure) | ux | P2 | M | medium |  |
| [PRODUCT-61](#product-61) | THEME — "Proxy as a product" (daemon + CLI + proactive health + notifications + smart routing + web dashboard) | feature | P2 (first pass P1) | XL | medium | partially-confirmed |
| [PRODUCT-64](#product-64) | THEME — "Platform SDK" (adapter trait, plugin surface for adapters/usage templates, new-app wave) | refactor | P2 | XL | medium |  |
| [PRODUCT-30](#product-30) | Proxy web dashboard (read-only status/logs served by the proxy) | feature | P3 | M | high |  |
| [PRODUCT-37](#product-37) | Tighten CSP and asset protocol scope | risk | P3 | S | high |  |
| [PRODUCT-38](#product-38) | Linux polish: in-app GDK backend setting, tray fallback detection, official Flatpak | ux | P3 | M | high |  |
| [PRODUCT-39](#product-39) | Updater channels (stable/beta) and in-app release notes | feature | P3 | S | high |  |
| [PRODUCT-41](#product-41) | Provider tags, groups and archive state | ux | P3 | S | high |  |
| [PRODUCT-42](#product-42) | Claude Code subagents & hooks management (replace the `AgentsPanel` stub) | feature | P3 | M | high |  |
| [PRODUCT-46](#product-46) | Localization expansion and translation tooling (de UI, key-coverage lint, community workflow) | feature | P3 | M | high |  |
| [PRODUCT-47](#product-47) | Opt-in, privacy-preserving crash reporting (no usage telemetry) | feature | P3 | S | medium |  |
| [PRODUCT-48](#product-48) | Session export (Markdown/JSON) and session analytics | feature | P3 | S | high |  |
| [PRODUCT-49](#product-49) | Model catalog sync across apps (one upstream model list → Codex catalog / OpenClaw models / OpenCode models / Claude mappings) | feature | P3 | M | medium |  |
| [PRODUCT-50](#product-50) | Usage data export (CSV/JSON) and scheduled monthly report | feature | P3 | S | high |  |
| [PRODUCT-52](#product-52) | Claude Desktop on Linux (3P profile writing) | feature | P3 | M | medium |  |
| [PRODUCT-53](#product-53) | Skills for OpenClaw and prompts for Hermes/OpenClaw | feature | P3 | S | medium |  |
| [PRODUCT-56](#product-56) | Quota/subscription lifecycle: reset countdown everywhere, expiry reminders, auto-refresh when inactive | feature | P3 | S | high |  |
| [PRODUCT-57](#product-57) | `/status` and request-detail: expose routing decisions and rectifier actions | ux | P3 | S | high |  |
| [PRODUCT-58](#product-58) | Provider terminal launch for all apps and "open with provider env" without writing live config | feature | P3 | S | high |  |
| [PRODUCT-59](#product-59) | Prompt library sharing: per-project prompt presets and multi-app prompts | feature | P3 | M | high |  |
| [PRODUCT-60](#product-60) | Dead/stub code cleanup: unreachable `agents` view, `api/vscode.ts` naming, unused `HealthChecker` | refactor | P3 | S | high |  |

## Findings

### PRODUCT-01: Secrets vault: OS keychain for API keys/tokens and encrypted export & cloud-sync bundles

**Type:** risk · **Priority:** P1 · **Effort:** L · **Confidence:** high · **Persona:** d, c, a

- **Evidence:** `src-tauri/Cargo.toml` has no `keyring`/`aes`/`chacha` crate (only `sha2`/`hmac` at lines 80-81); `services/subscription.rs:110-144` only *reads* Claude's own macOS Keychain entry; providers persist `settingsConfig` JSON (keys inline) in `providers` table (`database/schema.rs`); `export_config_to_file` (`commands/import_export.rs:21`) dumps SQL; WebDAV/S3 sync (`services/sync_protocol.rs`, `s3_sync.rs`) uploads manifest+payload with hash only; manual 2.1 says exports include "all provider configurations"; OAuth refresh tokens "stored in the local data directory" (manual 2.1 Codex OAuth).
- **Problem:** Every relay key, ChatGPT/Copilot refresh token and S3 secret sits in plaintext SQLite that is also copied into `~/.cc-switch/backups/`, `.sql` exports and cloud buckets; a stolen laptop backup or misconfigured bucket leaks all accounts.
- **Proposal:** MVP (1) store secrets via `keyring` crate (macOS Keychain / Windows Credential Manager / Secret Service) keyed by provider id, keep a `secret_ref` in DB with transparent fallback to file-based encrypted blob when no keystore; (2) export/sync bundles encrypted with a user passphrase (age/chacha20-poly1305, argon2 KDF) and an explicit "include secrets" switch; (3) migrate existing rows on first launch with backup. Depends on `provider.rs` serde model, `services/webdav_sync`/`s3_sync`, `import_export.rs`, live-write paths in `services/provider/live.rs` (must inject secrets at write time).
- **Acceptance:** `cc-switch.db` contains no raw `sk-`/bearer strings; exports without passphrase contain redacted placeholders; cloud sync round-trips on a second machine with passphrase; all existing tests pass; Linux without Secret Service degrades to encrypted file.
- **Verification:** **partially-confirmed** — Core holds: provider keys sit inline in providers.settings_config (database/schema.rs:31); export_config_to_file (commands/import_export.rs:21) calls export_sql (database/backup.rs:59-67) with no redaction; the sync artifact set is db.sql+skills.zip (services/sync_protocol.rs:3) built by export_sql_string_for_sync (backup.rs:53-56) which skips only log/health/rollup tables (backup.rs:19-25); integrity is SHA-256 only (sync_protocol.rs:130-137, 265-300); Cargo.toml:80-81 has just sha2/hmac (the aes in Cargo.lock is transitive from zip). Two evidence details are wrong: Codex OAuth refresh tokens are stored in codex_oauth_auth.json (proxy/providers/codex_oauth_auth.rs:238), not SQLite, so they never reach .sql exports or sync bundles (manual 2.1-add.md:467 'Token export is not supported'), and S3/WebDAV secrets live in settings.json (settings.rs:120,197) where get_settings_for_frontend (settings.rs:736-745) only masks them for the UI. subscription.rs:116-150 (and 487/801 for Codex/Gemini) only reads other apps' keychain entries, as stated. Priority lowered to P1: plaintext-at-rest needs a local/backup/bucket compromise and mirrors Claude Code's own .credentials.json, but pushing provider keys unencrypted into user exports and WebDAV/S3 buckets is a genuine high-severity exposure, and nothing implements the proposal.

### PRODUCT-02: Authenticate the proxy listener when bound to a non-loopback address

**Type:** risk · **Priority:** P1 · **Effort:** S · **Confidence:** high · **Persona:** d, b

- **Evidence:** manual 4.1 offers `0.0.0.0` "Allow LAN access"; `src/components/proxy/ProxyPanel.tsx:152` accepts `0.0.0.0`; `src-tauri/src/proxy/server.rs:292-368` has no auth layer; only the Claude Desktop route checks a gateway token (`proxy/handlers.rs:260`, `claude_desktop_config::get_or_create_gateway_token`).
- **Problem:** Anyone on the LAN (or on a shared Wi-Fi) can spend the user's relay credits and read model outputs by POSTing to `http://host:15721/v1/messages`; the proxy transparently attaches the user's upstream keys.
- **Proposal:** MVP: when listen address is not loopback, require a per-install bearer token (reuse `get_or_create_gateway_token`) on all routes except `/health`; show the token + "copy env snippet" in ProxyPanel; write it into takeover configs automatically (Claude `ANTHROPIC_AUTH_TOKEN` placeholder path already exists in `services/proxy.rs`); add a red warning banner for non-loopback binds. Optional allowlist CIDR.
- **Acceptance:** unauthenticated LAN request returns 401; local takeover flows keep working without user action; docs 4.1 updated.
- **Verification:** **confirmed** — ProxyPanel.tsx:152 accepts 0.0.0.0 with no warning, services/proxy.rs:1474-1479 explicitly maps a 0.0.0.0 bind to 127.0.0.1 for client configs (LAN binding is a supported path), and manual 4.1-service.md:56 advertises it as 'Allow LAN access'. build_router (proxy/server.rs:291-368) adds only a DefaultBodyLimit layer; the sole credential check in handlers.rs is validate_claude_desktop_gateway_auth (handlers.rs:255-281), used only by the /claude-desktop/* routes, so /v1/messages, /v1/chat/completions, /v1/responses and /v1beta/* take anonymous requests and inject the provider's upstream key. The proposal is sound and reuses existing pieces (get_or_create_gateway_token; the ANTHROPIC_AUTH_TOKEN placeholder path at services/proxy.rs:97-196). Priority corrected to P1: the default bind is loopback (proxy/types.rs:45), so exposure requires the user to opt in, although the docs invite that without any caution.

### PRODUCT-04: Proxy bridge conformance suite (golden fixtures per protocol pair)

**Type:** refactor · **Priority:** P1 · **Effort:** M · **Confidence:** medium · **Persona:** f, b

- **Evidence:** fix clusters in every release: v3.15.0 "Proxy Reliability Patches (P0-P3)", 10+ bridge fixes; v3.16.0 "Codex Chat Error Responses", "Mid-Stream System Messages", "Tool-Call Reasoning Backfill"; v3.16.2 stream truncation/tool_choice/custom tool metadata; v3.17.0 "Responses Upstream Failures No Longer Turn into Empty Replies", "Reasoning, Tool Results, and the System Role Preserved"; last 50 commits include `6d316c0`, `9ca1a41`, `613fef7`, `27ce0a5`, `650905a`. Transform code: `proxy/providers/transform_*.rs`, `streaming_*.rs` (10 files).
- **Problem:** Each new upstream quirk (MiniMax system messages, Kimi reasoning_content, DeepSeek thinking) is fixed point-wise and regresses elsewhere; contributors cannot verify a bridge change without live keys.
- **Proposal:** Record/replay fixture corpus (`src-tauri/tests/fixtures/bridge/<client>-<upstream>/*.json` request + SSE transcript) with a table-driven test that asserts round-trip equivalence (tool ids/order, reasoning blocks, usage deltas, error envelopes); add a "capture this request as fixture" developer toggle behind the existing log config; run in CI backend job (`.github/workflows/ci.yml:57-105`).
- **Acceptance:** ≥30 fixtures covering Anthropic→Chat/Responses/Gemini and Responses→Chat/Anthropic; every fix PR adds a fixture; CI red on regression.
- **Verification:** **partially-confirmed** — Churn evidence holds: docs/release-notes v3.15.0-en.md:223, v3.16.0-en.md:172/176/200, v3.17.0-en.md:167/171 and CHANGELOG.md:381-388,486 exist, and commits 613fef7, 6d316c0, 9ca1a41, 650905a, 27ce0a5 are all in git log. There is no fixture corpus (no fixtures dir, no include_str! under src-tauri/src/proxy or src-tauri/tests, no bridge test in src-tauri/tests/), and the CI backend job (.github/workflows/ci.yml:56-105) just runs cargo test. But 'contributors cannot verify a bridge change without live keys' is overstated: the 8 transform_*/streaming_* files (10 counting transform.rs/streaming.rs) hold ~294 inline #[test]s (67/55/24/74/20/19/14/21) that run keyless in CI; the real gap is golden round-trip fixtures per client/upstream pair. Priority corrected to P1: a test-infrastructure refactor is not P0 (nothing exploitable or data-losing), but the recurring user-facing bridge regressions justify high.

### PRODUCT-62: THEME — "Trust & governance" (vault, listener auth, signed catalog, disclosure, policy, audit, signing)

**Type:** risk · **Priority:** P1 · **Effort:** XL · **Confidence:** medium · **Persona:** d, c, e

- **Evidence:** PRODUCT-01/02/07/08/09/24/25/35/36/37/54.
- **Problem:** Growth (100k+ stars, imposter sites, paid-sponsor economy, ToS-sensitive features) has outpaced the trust infrastructure of a hobby-scale app.
- **Proposal:** Sequence: listener auth + README wording (02/09, days) → keychain + encrypted bundles (01) → signed catalog + disclosure + policy doc (07/08/54) → Windows signing/SBOM (35) → audit/time machine (25/21) → managed policy (24).
- **Acceptance:** a security review checklist (keys at rest, network exposure, supply chain, disclosure) passes with documented evidence.
- **Verification:** **partially-confirmed** — The underlying gaps I could verify are real: provider API keys live in plaintext JSON in SQLite (src-tauri/src/database/schema.rs:31 `settings_config TEXT`; Cargo.toml has no keyring/aes dependency and every 'encrypt' hit is about reasoning encrypted_content), the proxy's `/v1/messages` routes accept any local caller with no inbound auth while only the Claude Desktop gateway route validates a token (src-tauri/src/proxy/handlers.rs:128,144,256-281), the listen address defaults to 127.0.0.1 (proxy/types.rs:45) but 0.0.0.0 is an accepted configuration (services/proxy.rs:1474-1477), there is no Windows signing (PRODUCT-35) and no sponsor/affiliate disclosure (PRODUCT-54). P0 is not proportionate: nothing here is remotely exploitable or causes data loss in the default configuration (loopback-only listener, keys readable only by the same OS user, which is the same posture as ~/.claude.json itself), and 'signed catalog' is premature because presets are compiled into the binary (src/config/*Presets.ts; no remote fetch anywhere in src/lib/api). The '100k+ stars' figure and the sibling findings 01/02/07/08/24/25/36/37 could not be verified from the repository. The sequence (listener auth and README wording first, then keychain) is sound; as an umbrella it merits P1, not P0.

### PRODUCT-63: THEME — "Projects everywhere" (per-directory binding, all apps in profiles/tray/universal/takeover, usage by project)

**Type:** feature · **Priority:** P1 · **Effort:** XL · **Confidence:** high · **Persona:** b, a, c

- **Evidence:** PRODUCT-12/14/15/16/17/18; v3.17.0 positioned Projects as the headline but scope is 3 of 8 apps and manual-only.
- **Problem:** The mental model users want ("this repo uses these providers/MCP/skills") is only half-implemented.
- **Proposal:** Sequence: scope expansion (17) → tray/universal parity (15/16) → OpenCode/OpenClaw/Hermes takeover (18) → cwd binding via CLI hook/header (12) → project attribution in usage (14).
- **Acceptance:** switching directories switches tools' providers automatically; Usage answers per-repo cost.
- **Verification:** **partially-confirmed** — Observations hold: src-tauri/src/app_config.rs:370-384 defines 8 AppType variants while src-tauri/src/services/profile.rs:33-43 ProfileScope::ALL (mirrored in src/components/profiles/scope.ts:10-19) lists only Claude/ClaudeDesktop/Codex, and CHANGELOG.md:9 (3.17.0) states projects "cover Claude Code, Claude Desktop, and Codex". Manual-only is confirmed: rg for git_root/chpwd/per-project/X-CC-Switch-Project across src and src-tauri returns nothing, and docs/release-notes/v3.17.0-en.md:68 describes "when you switch directories to work, one click in CC Switch". Parity gaps verified: src-tauri/src/tray.rs:158-179 builds sections only for Claude/Codex/Gemini/GrokBuild (the Projects submenu iterates ProfileScope::ALL at tray.rs:777), src/types.ts:537-541 UniversalProviderApps is claude/codex/gemini, src/App.tsx:1250-1253 hides ProxyToggle/FailoverToggle for opencode/openclaw/hermes and opencode/openclaw/hermes_config.rs contain zero takeover references (only grok_config.rs:247 apply_proxy_takeover plus services/proxy.rs for Claude/Codex/Gemini/GrokBuild), and src-tauri/src/database/schema.rs:197-211 proxy_request_logs has session_id but no project column while src/components/usage has no project filter. The proposal's sequencing is off: PRODUCT-12 is the only P1 constituent and the one that satisfies the stated acceptance criterion, yet it is placed fourth behind three P2 items; its declared hard dependency PRODUCT-11 (a ccs CLI; src-tauri/Cargo.toml has no clap or [[bin]]) is omitted from the sequence; and PRODUCT-18 (proxy takeover for OpenCode/OpenClaw/Hermes) is only needed for 12's phase-2 header routing, not for profiles to cover those apps. P1 is proportionate as the follow-through on the headline 3.17.0 feature; this is a feature theme with no defect or data-loss exposure.

### PRODUCT-65: THEME — "Reliability engineering" (bridge fixtures, e2e scenarios, beta ring, diagnostics)

**Type:** refactor · **Priority:** P1 · **Effort:** L · **Confidence:** high · **Persona:** f, b

- **Evidence:** PRODUCT-04/45/39/29; fix-to-feature ratio in CHANGELOG 3.15-3.17 and 50 recent commits (25 `fix:` vs 16 `feat:`, 4 `docs:`).
- **Problem:** Weekly releases to 100k users with mock-only tests and one maintainer gate.
- **Proposal:** Fixture corpus + scenario harness in CI, beta channel, diagnostics bundle in issue template, release checklist that requires a fixture for every bridge fix.
- **Acceptance:** regression rate (fixes referencing a prior fix) drops measurably over two releases; beta catches at least one issue before stable.
- **Verification:** **partially-confirmed** — Evidence verified exactly: git log --format=%s -50 613fef7 yields 25 fix / 16 feat / 4 docs (plus one non-conventional "Fix/opencode known field editors"), CHANGELOG.md 3.15.0-3.17.0 has 176 Fixed vs 86 Added entries (3.15.0 alone 46 vs 15) across 8 releases in 58 days (2026-05-16 to 2026-07-13), and .github/CODEOWNERS requires @farion1231 on *, /.github/ and /src-tauri/ with 34 of ~41 non-merge commits in the last 300 by one author. None of the proposed items exist: no fixture dirs/files under src-tauri and [dev-dependencies] is only serial_test+tempfile (no mock-HTTP crate, no fake upstream in any test), package.json devDeps are msw+vitest with no Playwright/WebdriverIO/tauri-driver and .github/workflows/ci.yml runs only tsc/prettier/vitest plus fmt/clippy/cargo test, src-tauri/tauri.conf.json:64-66 has a single latest.json updater endpoint with no beta/channel strings in AboutSection.tsx/UpdateBadge.tsx, .github/ISSUE_TEMPLATE/bug_report.yml collects version/OS/app by hand with logs only under "Additional Context", and .github/pull_request_template.md has no test or fixture requirement (CONTRIBUTING.md:104 only says "You have tested it yourself"). The "mock-only tests" characterization is overstated: the backend has 2060 inline unit tests plus 114 integration tests in 12 files that use an isolated temp HOME with real file writes and real SQLite (src-tauri/tests/support.rs:7-24) and exercise takeover enter/restore (src-tauri/tests/provider_service.rs:532,1448,1547; profile_roundtrip.rs:647); the real gap is network-level (fake upstream, failover) and UI scenario coverage. The "100k users" figure cannot be verified from the repository (README.md:10 is only a downloads badge; the GitHub API is not reachable in this session), and .github/workflows/release.yml:612 already publishes every tag with prerelease: true, so a manual promote-to-latest step exists even though no user-facing beta ring does. P1 and effort L are proportionate for a weekly-shipping proxy app with this fix ratio and a single-owner gate; the proposal is sound and not implemented.

### PRODUCT-03: Proactive provider health probing (documented but unimplemented)

**Type:** bug · **Priority:** P2 · **Effort:** M · **Confidence:** high · **Persona:** b

- **Evidence:** manual 4.5 "Integration with Failover → Health Checks: After enabling the proxy service, the system periodically performs health checks on providers in the failover queue" and "Circuit Breaker Recovery: performs a model test"; code: `src-tauri/src/proxy/health.rs` is 7 lines (`pub struct HealthChecker;`); no `interval`/`spawn` health loop in `services/proxy.rs` or `services/stream_check.rs`; health is only passive (`provider_router.rs:126 record_result`) plus manual `stream_check_provider`.
- **Problem:** Users configure a failover queue expecting backups to be pre-validated; the first real request after an outage still pays the full timeout (60-90 s first-byte) before switching, and "All providers circuit-broken" (FAQ 5.2) recovers blindly.
- **Proposal:** MVP: background task in `ProxyServer` that, every N minutes (configurable, default 10), runs the existing lightweight `base_url` probe for queue members and a cheap `stream_check` for half-open breakers; feed results into `ProviderRouter::record_result`; expose last-probe time in `get_provider_health`; cap cost (max tokens 1-10) and skip official OAuth entries (already skipped per commit `f2c6d48`). Alternatively fix the docs if intentionally not shipped.
- **Acceptance:** a provider taken down returns red in the queue within one probe interval without any client traffic; half-open breaker closes after probe success; docs 4.5 matches behavior.
- **Verification:** **confirmed** — proxy/health.rs is a 7-line placeholder ('占位实现', pub struct HealthChecker;) while manual 4.5:106-121 promises periodic health checks on queue members and a model test on circuit recovery. No probe loop exists: the only tokio::spawn/sleep in services/proxy.rs, stream_check.rs, server.rs, provider_router.rs and failover_switch.rs are the listener task (server.rs:141-156) and test code; health is passive via record_result (provider_router.rs:126-147) and half-open permits (circuit_breaker.rs:315), and probe_reachability (stream_check.rs:150,198) runs only on user action. The frontend's 5-second refetchInterval (lib/query/failover.ts:13-17, 85-89) merely re-reads stored health/breaker stats. Priority corrected to P2: this is a docs/feature mismatch that slows first-request recovery, not an exploitable or data-loss defect, and the doc-fix alternative is trivial.

### PRODUCT-05: Finish budget limits & alerts (backend exists, UI/enforcement missing)

**Type:** feature · **Priority:** P2 · **Effort:** M · **Confidence:** high · **Persona:** a, b

- **Evidence:** `src-tauri/src/provider.rs:437-440` (`limitDailyUsd`/`limitMonthlyUsd` meta), `services/usage_stats.rs:1599 check_provider_limits`, `commands/usage.rs:252`, FE `src/lib/api/usage.ts:171` + `lib/query/usage.ts:352`; no form field writes these keys (grep: only types/api/query files) and nothing consumes `dailyExceeded`.
- **Problem:** Relay users top up small balances and routinely discover overspend after the fact; the dashboard is retrospective only.
- **Proposal:** MVP: add daily/monthly USD limit fields to the provider form (Advanced), show a progress pill on the card, and act on breach: (1) toast + tray notification (PRODUCT-06), (2) optional "skip in failover when exceeded" in `ProviderRouter::allow_provider_request`, (3) global monthly budget in Usage tab with projection line on `UsageTrendChart`. Depends on `usage_daily_rollups` for cheap sums.
- **Acceptance:** setting a $1 daily limit on a provider and exceeding it via proxy produces an alert within 60 s and (if enabled) routes to the next queue member.
- **Verification:** **confirmed** — Confirmed end to end: limitDailyUsd/limitMonthlyUsd in ProviderMeta (provider.rs:436-441), check_provider_limits reading them from providers.meta (services/usage_stats.rs:1599-1625), the Tauri command (commands/usage.rs:251-258), usageApi.checkProviderLimits (lib/api/usage.ts:171-176) and useProviderLimits (lib/query/usage.ts:349-356). A repo-wide grep for those keys hits only src/types/usage.ts, src/lib/api/usage.ts and src/lib/query/usage.ts on the frontend; no .tsx component calls useProviderLimits and no form writes the keys, so the feature is dead code. Proposal dependencies exist (allow_provider_request at provider_router.rs:119, the usage_daily_rollups table, src/components/usage/UsageTrendChart.tsx). Priority corrected to P2: a missing feature with bounded financial impact, not a security or data-loss issue.

### PRODUCT-06: System/tray notifications for failover, circuit open, budget, quota exhaustion and sync errors

**Type:** feature · **Priority:** P2 · **Effort:** M · **Confidence:** high · **Persona:** a, b

- **Evidence:** no `tauri-plugin-notification` (Cargo.toml plugin list lines 30-38, 87); backend emits only `provider-switched`, `usage-cache-updated`, `proxy-flags-changed`, `deeplink-import`, `webdav/s3-sync-status-updated`; failover switches are logged (manual 4.3 "Failover Logs") but surfaced only inside the usage logs; `proxy-official-warning` is a toast visible only when the window is open; Lightweight Mode destroys the window (manual 1.5).
- **Problem:** In tray-only mode a user whose primary provider trips the breaker, whose Codex OAuth session expires, or whose Zhipu 5-hour bucket hits 100% learns nothing until a tool errors.
- **Proposal:** Add `tauri-plugin-notification`; emit typed events (`failover-switched`, `circuit-opened/closed`, `quota-threshold`, `budget-exceeded`, `sync-failed`, `session-expired`) from `failover_switch.rs`, `provider_router.rs`, `services/usage_cache.rs`, subscription/coding-plan services; per-event toggles under Settings → General; respect OS do-not-disturb; dedupe with 60 s window.
- **Acceptance:** with window closed, a forced upstream 500 storm produces exactly one OS notification naming old/new provider; toggles persist in `settings.json`.
- **Verification:** **confirmed** — No tauri-plugin-notification appears in src-tauri/Cargo.toml (plugin block lines 31-38), package.json, tauri.conf.json, capabilities/ or lib.rs. The backend's .emit( call sites name exactly deeplink-import, provider-switched, proxy-flags-changed, s3-sync-status-updated, usage-cache-updated, webdav-sync-status-updated, plus proxy-official-warning (services/proxy.rs:838-842) which App.tsx:436 turns into an in-window toast. manual 1.5-settings.md:54 confirms Lightweight Mode destroys the main window, manual 4.3:170 keeps failover logs in-app, and tray.rs (503-552) only handles the Auto click with no health/failover indication. Priority corrected to P2: a notifications feature gap, not exploitable or data-loss.

### PRODUCT-07: Remote, signed preset & sponsor catalog (decouple commercial changes from app releases)

**Type:** feature · **Priority:** P2 · **Effort:** L · **Confidence:** medium · **Persona:** e, a

- **Evidence:** all presets hardcoded in `src/config/*ProviderPresets.ts` (11,227 lines) with `isPartner`/`primePartner`/`partnerPromotionKey` (`claudeProviderPresets.ts:25-80`) and promo copy in `src/i18n/locales/*.json:1018`; churn: "Removed LionCC Sponsor and Presets", "AICoding Partner Entry" removed (v3.16.0), "MiniMax Demoted", "LemonData Removed, SudoCode Demoted" (v3.16.3), SudoCode re-promoted (`618723b`), endpoint/link edits in nearly every release (v3.15.0 "Partner Links Refreshed", v3.16.5 "Volcengine ... links corrected", v3.17.0 reverted).
- **Problem:** Every sponsor onboarding, demotion, endpoint change or affiliate-link fix needs a full release and a 4-locale README edit; users on older versions keep dead endpoints; presets cannot be updated for a provider outage.
- **Proposal:** Ship presets as a versioned JSON catalog (`catalog/v1/<app>.json` + `partners.json`) signed with the existing minisign key (`tauri.conf.json` updater pubkey); app bundles a snapshot and fetches updates daily (opt-out), verifying signature before use; UI shows catalog version and "updated X ago"; keep TypeScript types as the schema source. Depends on `ProviderPresetSelector.tsx`, model-fetch, i18n for promo text (move to catalog with per-locale strings).
- **Acceptance:** adding/removing a partner is a PR to the catalog repo/dir only; a tampered catalog is rejected; offline users keep bundled snapshot; preset list identical to today after migration.
- **Verification:** **partially-confirmed** — Hardcoding and churn confirmed: isPartner/primePartner/partnerPromotionKey at claudeProviderPresets.ts:32-34, promo copy at i18n/locales/en.json:1018-1026, no remote fetch anywhere (the only 'catalog' hit is a comment at claudeProviderPresets.ts:1239), minisign updater pubkey at tauri.conf.json:63, and CHANGELOG.md:37,39,104,213-215,364,375,416-417 plus commit 618723b document the sponsor/link churn. The size figure is off: src/config/*ProviderPresets.ts total 10,746 lines (11,099 for all src/config/*.ts), not 11,227. Priority corrected to P2: this is maintainer velocity and stale links on old builds rather than user harm, and the proposal introduces a signed remote-update surface plus a daily network call that needs opt-out and review, so it is sound but heavier than framed.

### PRODUCT-08: In-app affiliate/sponsor disclosure and preset provenance

**Type:** risk · **Priority:** P2 · **Effort:** S · **Confidence:** high · **Persona:** e, d

- **Evidence:** partner presets render only a `Star`/`Heart` badge (`ProviderPresetSelector.tsx:404-432`) and are sorted first (v3.15.0 "partners prioritized first"); `websiteUrl`/`apiKeyUrl` carry affiliate params (e.g. `?aff=cc-switch`, `invitecode=9915W3`, `aff=WB6M6F67` in `claudeProviderPresets.ts:108-130`); no i18n key mentions affiliate/referral (grep of `en.json`); README has a sponsor section but the app does not.
- **Problem:** Users cannot tell that "recommended" ordering and "Get API Key" links are paid placements; trust is the project's main asset (imposter-site warnings in every release note).
- **Proposal:** Tooltip + footer line in the preset selector ("Sponsored placement; CC Switch may earn a referral commission"), a Settings → About "Sponsors & affiliate policy" panel listing partner presets with their link targets, and a "Hide sponsored presets / don't prioritize" preference. Pair with PRODUCT-07 so disclosure text travels with the catalog.
- **Acceptance:** every `isPartner` preset shows the disclosure on hover; preference persists; no change to link targets.
- **Verification:** **partially-confirmed** — Visuals and links confirmed: ProviderPresetSelector.tsx:404-434 renders only a Heart (primePartner) or Star (isPartner) badge with a title that is just the category label (414-417); claudeProviderPresets.ts:116 (aff=WB6M6F67) and 130-131 (ac=...&rc=6J6FV5N2&utm_*) carry referral params; en.json has no affiliate/referral/sponsor/commission string; no hide/deprioritize preference exists; README.md:21 is the only sponsor section. Two details are off: the selector's sort (lines 90-105) programmatically promotes only official and primePartner entries, regular partners come first only by file order; and ApiKeySection.tsx:76-80 does show the partnerPromotion copy after selection, which for some partners reads 'official partner of CC Switch'/'promo code cc-switch' (en.json:1019-1026), a partial disclosure, though nothing mentions commissions or paid ordering and nothing is shown before selection. Priority corrected to P2: a transparency/compliance concern, not security or data loss.

### PRODUCT-09: ToS-sensitive feature gate and README wording fix ("signature bypass")

**Type:** risk · **Priority:** P2 · **Effort:** S · **Confidence:** medium · **Persona:** d, e

- **Evidence:** README "Built-in Utilities — ... first-launch login confirmation, signature bypass, plugin extension sync"; `README_ZH.md:209` says 禁止签名 which maps to the *thinking-signature rectifier* (`src/i18n/locales/zh.json:381 "thinkingSignature": "Thinking 签名整流"`, `proxy/thinking_rectifier.rs`), not code-signing or auth bypass; reverse-proxy features with explicit ToS risk notices: Copilot (v3.12.3), Codex OAuth (v3.13.0), official ChatGPT takeover (v3.17.0 "same class of usage"); manual 2.1 "Risk Notice (Important)".
- **Problem:** The English README advertises a "signature bypass" that does not exist, which reads as a ToS/security circumvention claim to enterprises and platform vendors; the real ToS-risk features are reachable with a one-time dialog only.
- **Proposal:** (1) Reword README EN/JA/DE to "thinking-signature rectifier"; (2) add a single "ToS-sensitive features" switch (default off on fresh installs, on for existing users who already confirmed) that hides Copilot/Codex-OAuth/official-takeover entries and shows the risk notice once; (3) make it policy-controllable (PRODUCT-24).
- **Acceptance:** README no longer contains "signature bypass"; fresh install shows no reverse-proxy presets until the switch is enabled; existing users unaffected.
- **Verification:** **partially-confirmed** — The wording exists in all four READMEs (README.md:208 'signature bypass', README_DE.md:208 'Umgehen von Signaturen', README_JA.md:208 '署名バイパス', README_ZH.md:209 '禁止签名'), and the thinking rectifier exists (src/i18n/locales/zh.json:381 'Thinking 签名整流', src-tauri/src/proxy/thinking_rectifier.rs:1-4). However the reviewer's mapping is unproven: the app names the rectifier '整流' not '禁止', and the default common-config snippet is `{"includeCoAuthoredBy": false}` (src/components/providers/forms/hooks/useCommonConfigSnippet.ts:11-13), i.e. suppressing Claude Code's Co-Authored-By commit 'signature', which is the more literal reading of '禁止签名' next to the other listed utilities (settings.rs:356 skip_claude_onboarding, enable_claude_plugin_integration), so the proposed rewording to 'thinking-signature rectifier' may itself be wrong. The claim that ToS features sit behind 'a one-time dialog' is inaccurate in the other direction: there is no in-app risk dialog at all (no acknowledge/risk/dialog logic in src/components/settings/AuthCenterPanel.tsx, CopilotAuthSection.tsx, CodexOAuthSection.tsx, useCopilotAuth.ts, useCodexOauth.ts); the only in-app text is en.json:326 'please be mindful of compliance risks', with notices confined to release notes (v3.12.3-en.md:249-259, v3.17.0-en.md:272-282) and the manual (2.1-add.md:491-500), while the Copilot/Codex presets sit in the ordinary third_party list (claudeProviderPresets.ts:1208-1253, claudeDesktopProviderPresets.ts:378-396). A README wording fix plus an opt-in gate is a policy/reputation item, not an exploit, so P2.

### PRODUCT-10: Headless proxy daemon (proxy survives app quit / runs without window)

**Type:** feature · **Priority:** P2 · **Effort:** L · **Confidence:** medium · **Persona:** b, c

- **Evidence:** proxy lives in the Tauri process (`proxy/server.rs:141 tokio::spawn` inside app state); manual 2.6 "Model mapping mode: CC Switch must keep running"; v3.15.0 "quitting CC Switch or stopping Local Routing will cut off any proxy-mode Claude Desktop providers"; v3.16.3 "Takeover-Residue Recovery" fixes for tools pointed at a dead local proxy; no `clap`/daemon code (grep).
- **Problem:** Takeover rewrites the CLI's base URL to `127.0.0.1:15721`; any crash/quit/update leaves Claude Code, Codex, Gemini and Claude Desktop pointed at nothing, and CI/SSH/headless Linux boxes cannot use failover at all.
- **Proposal:** Extract `proxy` + `services/proxy.rs` + DB access into a `cc-switch-core` crate; add a `ccs-proxyd` binary (same DB, same settings) with `start/stop/status`, launchd/systemd/Task Scheduler install helpers from Settings → Proxy; the GUI becomes a client of the daemon when present (status via `/status`). Prerequisite for PRODUCT-11 and PRODUCT-30.
- **Acceptance:** quitting the GUI keeps `curl :15721/health` green and Claude Code requests flowing; daemon restarts on login; GUI shows "managed by daemon" state.
- **Verification:** **partially-confirmed** — Evidence holds: the listener is spawned inside the Tauri process (src-tauri/src/proxy/server.rs:141 `tokio::spawn`), the manual says model-mapping mode needs CC Switch running (docs/user-manual/en/2-providers/2.6-claude-desktop.md:295), v3.15.0-en.md:445 warns that quitting cuts off proxy-mode providers, CHANGELOG.md:225 documents takeover-residue recovery, and there is no CLI/daemon (Cargo.toml has only `auto-launch`; the sole 'systemd' hit is a comment at commands/misc.rs:2247; single [[bin]] 'cc-switch'). The impact statement is overstated: a normal quit runs cleanup_before_exit (src-tauri/src/lib.rs:1689-1713) which calls stop_with_restore_keep_state, so Claude Code/Codex/Gemini are restored to direct provider configs rather than left 'pointed at nothing'; the restart/update path deliberately skips restore because the new instance re-takes over (lib.rs:1544-1560 and restore_proxy_state_on_startup at lib.rs:1068/1770); only a crash leaves residue, which is repaired on next start. The app is also already tray-resident without a window (settings.rs:497-509 minimize_to_tray_on_close defaults true, silent_startup at settings.rs:362, launch_on_startup at :359), and `/health` and `/status` routes already exist (server.rs:294-295), so 'daemon restarts on login' and 'status via /status' are largely covered for GUI hosts; the genuinely unmet case is headless Linux/CI/SSH. A large feature with a working tray-background fallback is P2, not P1.

### PRODUCT-11: CLI companion (`ccs`) over the same SQLite store

**Type:** feature · **Priority:** P2 · **Effort:** L · **Confidence:** high · **Persona:** b, c, f

- **Evidence:** no CLI entrypoint (`src-tauri/src/main.rs` only launches Tauri; no `clap` in Cargo.toml); all operations are Tauri commands (`commands/*.rs`) backed by `AppState`/`Database`; deep links are the only external automation (`deeplink/parser.rs:59-64`, import only).
- **Problem:** Scripts, dotfiles, tmux workflows and remote shells cannot switch providers, apply a project, or start the proxy; the FAQ workaround is editing JSON by hand which defeats the SSOT.
- **Proposal:** `ccs` binary (shares core crate from PRODUCT-10): `ccs ls [app]`, `ccs switch <app> <provider>`, `ccs profile apply <name> --scope codex`, `ccs proxy start|stop|status`, `ccs usage today`, `ccs export --no-secrets`, `ccs doctor` (env conflicts + tool versions), `--json` output; bundle it in installers and add to PATH on macOS (`/usr/local/bin` symlink prompt) and Windows (MSI path feature).
- **Acceptance:** `ccs switch claude packycode` updates `~/.claude/settings.json` identically to the GUI and the GUI reflects it (via `provider-switched` or file watch) within 2 s.
- **Verification:** **partially-confirmed** — Confirmed there is no CLI surface: src-tauri/src/main.rs only sets Linux WebKit env vars and calls cc_switch_lib::run(); src-tauri/Cargo.toml has neither `clap` nor `tauri-plugin-cli`; deep links are import-only (src-tauri/src/deeplink/parser.rs:59-64 dispatches resource=provider/prompt/mcp/skill into DeepLinkImportRequest; deeplink/mod.rs:3 'for importing configurations'); scripts/ holds only icon helpers. The acceptance is realistic since the `provider-switched` event already exists (src-tauri/src/tray.rs:584-586). One detail is off: the manual does not merely tolerate hand-editing as a workaround that 'defeats the SSOT' - docs/user-manual/en/5-faq/5.1-config-files.md:303-321 marks live CLI config files 'Safe to Edit Manually' and describes backfill into the database on next edit, so manual edits are a documented, partially reconciled path. This is a feature request for scripting/remote personas with no data-loss angle, so P2 with effort L is proportionate.

### PRODUCT-12: Per-project (cwd / git repo) automatic profile binding

**Type:** feature · **Priority:** P2 · **Effort:** L · **Confidence:** medium · **Persona:** b, a

- **Evidence:** Projects are global snapshots switched manually (`services/profile.rs:5` "按分组（scope）操作", `ProfileSwitcher.tsx`, tray `profile_<scope>_<uuid>` events in `tray.rs:386`); no cwd/git detection anywhere (grep `git_root|per.?project` empty); v3.17.0 release notes motivate Projects with "one directory for coding and another for writing".
- **Problem:** The headline 3.17.0 use case ("a Development set for a coding directory") still requires a manual click every time the user changes directory; forgetting it silently spends the wrong provider.
- **Proposal:** MVP: `.ccswitch.toml` in a repo root (`profile = "work"`, optional per-app provider) + `ccs hook` shell integration (zsh/bash/fish `chpwd`) and a "Open terminal here" path (existing `open_provider_terminal`) that applies the profile; phase 2: the proxy reads a `X-CC-Switch-Project` header injected by the hook via `ANTHROPIC_CUSTOM_HEADERS`/Codex `http_headers` so routing is per-session without rewriting files. Depends on PRODUCT-11 and `ProfileService::apply`.
- **Acceptance:** `cd ~/work/api && claude` uses the "work" provider while another terminal in `~/blog` uses "writing" at the same time under takeover.
- **Verification:** **partially-confirmed** — Projects are indeed global, manually applied snapshots scoped by app group: src-tauri/src/services/profile.rs:3-7 and the ProfileScope enum at profile.rs:33-38 (Claude/ClaudeDesktop/Codex), tray events `profile_<scope>_<uuid>` at src-tauri/src/tray.rs:381-386, the popover UI in src/components/profiles/ProfileSwitcher.tsx, and v3.17.0-en.md:40,68 motivating 'a Development set for a coding directory'; no cwd/git-root detection exists (grep finds only the terminal-launch cwd in commands/misc.rs:2606-2609 and panic_hook.rs:53). The reviewer missed an existing partial mitigation: `open_provider_terminal` (commands/misc.rs:2602-2628, exposed via src/lib/api/providers.ts:143-153 and ProviderActions.tsx:345) launches a terminal with that provider's env vars and an optional cwd, so two terminals can already use different providers concurrently without rewriting settings.json - that covers the concurrency half of the acceptance, though not automatic per-directory binding or routing under takeover. Phase 2 is unimplemented (proxy/session.rs extracts session IDs but proxy/provider_router.rs has no header-based routing) and phase 1 depends on a `ccs hook` CLI that does not exist (PRODUCT-11). Realistic impact is accidental spend on the wrong provider, so P2.

### PRODUCT-13: Model-aware "smart routing" (cheapest/fastest/healthiest provider per model, model fallbacks)

**Type:** feature · **Priority:** P2 · **Effort:** L · **Confidence:** high · **Persona:** b, a

- **Evidence:** `proxy/provider_router.rs:37 select_providers(app_type)` chooses only the app's current provider plus failover queue order; no latency/cost/model terms (grep empty); pricing per model exists (`model_pricing` table, `costMultiplier` meta `src/types.ts:190`), latency per request exists (`proxy_request_logs.latency_ms`, `first_token_ms`), and `/v1/models` catalogs are known per provider (`fetch_models_for_config`).
- **Problem:** Users with 3-5 relays cannot say "send haiku-class calls to the cheapest healthy relay, opus to the official key" or "if provider X lacks model Y, fall back to Z"; failover is ordinal only.
- **Proposal:** Routing rules table per app: match on requested model (glob) → ordered candidate list with strategy `priority|cheapest|lowest-latency(p50 over 1h)|round-robin`; candidates filtered by breaker state and model availability; exposed in Settings → Proxy → Routing with a dry-run tester; audit the decision in request detail (`RequestDetailPanel`). Depends on PRODUCT-03 for fresh health.
- **Acceptance:** a rule "claude-haiku-* → cheapest" routes to the lowest `input_price*multiplier` healthy provider and the request log names the rule that fired.

### PRODUCT-14: Usage attribution by project/session and per-session cost in the Session Manager

**Type:** feature · **Priority:** P2 · **Effort:** M · **Confidence:** high · **Persona:** a, c

- **Evidence:** `proxy_request_logs` has `session_id` but no project/cwd column (`database/schema.rs:197-212`); dashboard filters are date/app/provider/model only (manual 4.4, v3.16.3 "Dashboard-Wide Provider / Model Filters"); session-log sync already parses Claude/Codex/Gemini/OpenCode JSONL/SQLite that carry the project directory (session manager shows "Project directory").
- **Problem:** Freelancers and team leads cannot answer "how much did repo X cost this month" or see a session's cost next to its transcript.
- **Proposal:** Add `project_path` (normalized git root) to usage rows from session-log sync and, under takeover, from the header in PRODUCT-12; new "Project" filter + table in UsageDashboard; show cost/tokens badge in `SessionItem`/detail header; CSV export (PRODUCT-50).
- **Acceptance:** Usage tab groups by project with totals that reconcile with provider totals; a Claude session detail shows its cost.

### PRODUCT-15: Tray parity for all eight apps plus proxy/failover controls

**Type:** ux · **Priority:** P2 · **Effort:** M · **Confidence:** high · **Persona:** a, b

- **Evidence:** `tray.rs` builds submenus only for `AppType::Claude|Codex|Gemini|GrokBuild`; manual 1.3 "The tray currently focuses on Claude / Codex / Gemini"; tray has an "Auto (Failover)" entry (`tray.rs:105,503`) but no proxy start/stop, no Claude Desktop/OpenCode/OpenClaw/Hermes.
- **Problem:** Lightweight Mode users of OpenCode/OpenClaw/Hermes/Claude Desktop must open the window for every switch.
- **Proposal:** Generate submenus from `AppType::all()` honoring `visible_apps`; add "Local routing: on/off" and "Failover: on/off" check items per supported app; show profile for all scopes once PRODUCT-17 lands; keep menu depth ≤2.
- **Acceptance:** every visible app with ≥1 provider has a tray submenu; switching OpenClaw default model from tray works.

### PRODUCT-16: Universal providers for OpenCode/OpenClaw/Hermes/Grok Build

**Type:** feature · **Priority:** P2 · **Effort:** M · **Confidence:** high · **Persona:** a, e

- **Evidence:** `UniversalProviderApps` = claude/codex/gemini (`src/types.ts:537-541`; `UniversalProviderFormModal.tsx:53-55`); partner presets exist for the other apps separately (e.g. CCSub "covers six apps" v3.16.2), so one relay key is entered up to six times.
- **Problem:** Relay users with one key for everything re-enter and re-rotate the same key per app; a key change means six edits.
- **Proposal:** Extend apps/models maps to the five remaining apps, reuse per-app projection code paths in `services/provider/mod.rs` (`sync_universal_provider`), add per-app model fields (OpenCode SDK package, OpenClaw `api` kind, Hermes `custom_providers`), and a "promote this app-provider to universal" action.
- **Acceptance:** a NewAPI universal provider syncs to all eight apps and a key edit propagates in one save.

### PRODUCT-17: Extend Projects (profiles) scope to Gemini/Grok Build/OpenCode/OpenClaw/Hermes

**Type:** feature · **Priority:** P2 · **Effort:** M · **Confidence:** high · **Persona:** b

- **Evidence:** `services/profile.rs:42-46` `ProfileScope::ALL = [Claude, ClaudeDesktop, Codex]`; `src/components/profiles/scope.ts` mirrors; payload captures providers/mcp/skills/prompts per app (`src/lib/api/profiles.ts`).
- **Problem:** A "project" cannot carry a Gemini or OpenCode provider/MCP set, so multi-tool users still switch half by hand.
- **Proposal:** Add scopes for the remaining apps (Gemini/GrokBuild straightforward: provider+MCP+skills+prompt; OpenCode/OpenClaw/Hermes: default model + MCP + skills), keep per-scope slots, extend tray/header switcher.
- **Acceptance:** a profile applied on the Gemini tab switches provider+MCP+skills; other scopes untouched.

### PRODUCT-18: Proxy takeover (routing/failover/usage) for OpenCode, OpenClaw and Hermes

**Type:** feature · **Priority:** P2 · **Effort:** L · **Confidence:** medium · **Persona:** b, e

- **Evidence:** `services/proxy.rs` references only Claude/Codex/Gemini/GrokBuild; `App.tsx` hides `ProxyToggle` for opencode/openclaw/hermes; Grok Build takeover was added via `grok_config.rs:247 apply_proxy_takeover` (base_url rewrite + placeholder), showing the pattern; OpenClaw providers have `baseUrl` (manual 5.1), Hermes `custom_providers` have base URLs, OpenCode providers use `@ai-sdk/*` packages with `baseURL` options.
- **Problem:** Three of eight apps get no failover, no usage logging via proxy (OpenCode has session sync only), and no format conversion.
- **Proposal:** Implement `apply_proxy_takeover`/restore for the three adapters by rewriting each live provider's base URL to the proxy with a `provider_id`-scoped path (`/openclaw/<id>/v1/...`) so multiple providers can coexist (these apps are "additive" multi-provider configs); reuse `ProviderRouter` and usage logger; gate behind a per-app toggle.
- **Acceptance:** OpenClaw request through takeover appears in request logs with provider/model/cost; disabling restores the original JSON5 byte-for-byte except the rewritten fields.

### PRODUCT-19: Command palette and global hotkeys

**Type:** ux · **Priority:** P2 · **Effort:** M · **Confidence:** high · **Persona:** a, b

- **Evidence:** shortcuts today are only Cmd/Ctrl+, (`App.tsx:602`), Cmd/Ctrl+F (`ProviderList.tsx:252`), Esc; `cmdk` is bundled (`package.json`, `ui/command.tsx`) but used only in `OmoFormFields.tsx` and `ProfileSwitcher.tsx`; no `tauri-plugin-global-shortcut`.
- **Problem:** Power users switching several times a day navigate app switcher → card → Enable; no way to switch from another app without the mouse.
- **Proposal:** Cmd/Ctrl+K palette listing "Switch <app> → <provider>", "Apply project", "Toggle routing", "Open Usage", "Run stream check"; add `tauri-plugin-global-shortcut` with a configurable global hotkey that shows the palette (window rebuild path from Lightweight Mode already exists); document in manual 1.3.
- **Acceptance:** palette actions cover all tray actions; global hotkey works while another app is focused on macOS/Windows/Linux-X11 (Wayland limitation documented).

### PRODUCT-20: Deep links for navigation and actions (`ccswitch://open/...`, `ccswitch://switch?...`)

**Type:** feature · **Priority:** P2 · **Effort:** S · **Confidence:** high · **Persona:** c, f, e

- **Evidence:** parser accepts only `resource=provider|prompt|mcp|skill` imports (`deeplink/parser.rs:59-64`); handler emits only `deeplink-import` (`lib.rs:131-160`); docs 5.3 list import use cases only.
- **Problem:** Tutorials, sponsor docs and team wikis cannot link to "open Usage tab" or "switch to provider X"; scripts cannot trigger a switch without the CLI.
- **Proposal:** `ccswitch://v1/open?view=usage|proxy|mcp|skills|settings.tab`, `ccswitch://v1/switch?app=claude&provider=<id|name>` and `ccswitch://v1/profile/apply?name=...`, all behind a confirmation dialog for mutating actions (reuse `DeepLinkImportDialog`); extend `deplink.html` generator.
- **Acceptance:** links from the generator open the right view; mutating links prompt once and apply; invalid ones surface `deeplink-error`.

### PRODUCT-21: Config time machine: per-entity change history with diff and rollback

**Type:** feature · **Priority:** P2 · **Effort:** M · **Confidence:** high · **Persona:** a, c, d

- **Evidence:** backups are whole-DB only (`import_export.rs` create/list/restore/rename/delete, auto backup `backup_interval_hours`/`backup_retain_count` in `settings.rs:475-478`, 10-backup rotation per README FAQ); no diff UI (grep `diff` in `BackupListSection.tsx` empty); the JSON editor backfills live edits silently (manual 2.3 "backfill").
- **Problem:** Restoring a whole DB to undo one bad provider edit also reverts MCP/skills/usage pricing; users cannot see what backfill changed.
- **Proposal:** `config_history` table (entity type, id, JSON before/after, source = ui|backfill|deeplink|sync|cli, ts); "History" button on provider/MCP/prompt with side-by-side JSON diff (reuse CodeMirror merge view) and "Restore this version"; retention setting. Feeds PRODUCT-25 (audit).
- **Acceptance:** editing a provider three times yields three diffs; rollback restores exactly and re-projects to live file when active.

### PRODUCT-22: Share-safe export and import preview (redaction, selective scope, dry-run)

**Type:** feature · **Priority:** P2 · **Effort:** S · **Confidence:** high · **Persona:** c, e

- **Evidence:** export is a full `.sql` dump (manual 2.1 "Database Backup Import"; `export_config_to_file`), import "will overwrite the existing database"; no redact/exclude options (grep in `ImportExportSection.tsx` empty).
- **Problem:** Team leads cannot share a provider/MCP/prompt bundle without leaking keys or clobbering a colleague's DB.
- **Proposal:** JSON bundle format with scope checkboxes (providers/MCP/prompts/skills repos/pricing), "strip secrets" default on, import as merge with conflict preview (reuse deep-link confirmation components); keep SQL for full backup.
- **Acceptance:** exported bundle without secrets imports on a second machine and prompts only for missing keys.

### PRODUCT-23: MCP server health check, tool listing and richer catalog

**Type:** feature · **Priority:** P2 · **Effort:** M · **Confidence:** high · **Persona:** a, b

- **Evidence:** only `validate_mcp_command` (checks the executable exists, `commands/mcp.rs:40`); no test/ping in `UnifiedMcpPanel.tsx`/`McpFormModal.tsx` (grep); 5 presets in `src/config/mcpPresets.ts`; manual 3.1 FAQ "Server Fails to Start → check command/PATH".
- **Problem:** A misconfigured stdio server fails silently inside the CLI; users cannot see which tools a server exposes before enabling it across six apps.
- **Proposal:** "Test" action that spawns stdio (or GETs http/sse) with MCP `initialize` + `tools/list`, shows tools/latency/errors, and marks the row healthy; weekly background check with notification (PRODUCT-06); import catalog from the official MCP registry API into a searchable list (like skills.sh search) instead of 5 presets.
- **Acceptance:** a typo in `args` is caught by Test; registry search returns ≥100 servers and installs with correct transport.

### PRODUCT-24: Team policy ("managed mode") file for shared machines and orgs

**Type:** feature · **Priority:** P2 · **Effort:** L · **Confidence:** medium · **Persona:** c, d

- **Evidence:** no policy/lock concept (grep `policy` empty); settings are per-device JSON (`settings.rs`); cloud sync is bidirectional single-user (`sync_protocol.rs`); v3.17.0 notes cite "plenty of companies ban the Claude Code client ... but not the Claude API".
- **Problem:** Companies that allow the API but not certain clients/relays cannot enforce "only these endpoints, proxy mandatory, no reverse-proxy features, these MCP servers pinned".
- **Proposal:** `policy.json` (path or signed URL, admin-installed) with allow/deny endpoint globs, forced takeover, disabled feature flags (ToS-sensitive, deep-link import, cloud sync), pinned MCP/skills, and read-only presets; UI shows a "Managed by policy" badge and disables controls; CLI `ccs doctor` reports policy state.
- **Acceptance:** with a policy denying `*.example-relay.com`, adding such a provider is blocked with the policy reason; policy signature failure ignores the file.

### PRODUCT-25: Local audit log of configuration mutations and proxy admin actions

**Type:** feature · **Priority:** P2 · **Effort:** M · **Confidence:** high · **Persona:** d, c

- **Evidence:** no audit concept (grep `audit` empty); app log is level-configurable text (`LogConfigPanel.tsx`, `~/.cc-switch/logs/`); backfill and deep-link imports mutate config without a persistent record.
- **Problem:** Security reviews cannot answer "when did the base URL change and from where (UI, deep link, cloud sync, backfill)?".
- **Proposal:** Append-only `audit_events` table (actor=local user/cli/deeplink/sync, action, entity, before/after hash, ts) written from `services/provider`, `mcp`, `prompt`, `skill`, `proxy`, `deeplink`; Settings → Advanced → Audit viewer with export; shares storage with PRODUCT-21.
- **Acceptance:** every mutation path produces exactly one event; export is JSONL.

### PRODUCT-26: Import from adjacent tools' configs (claude-code-router, one-api/new-api, Cline/Roo, Continue, Aider)

**Type:** feature · **Priority:** P2 · **Effort:** M · **Confidence:** medium · **Persona:** a, e

- **Evidence:** imports exist only for the app's own live config (`import_default_config`, `import_*_providers_from_live`) and deep links; no mention of claude-code-router/ccr/one-api/cline/continue in `src/config` or components (grep hits were only `cursor-pointer` CSS).
- **Problem:** Users migrating from claude-code-router (`~/.claude-code-router/config.json` Providers[]/Router), new-api channel exports, Cline/Roo provider profiles or Continue `config.yaml` retype dozens of endpoints/keys.
- **Proposal:** "Import from…" dialog with format detectors and mappers to `Provider`/universal providers (and claude-code-router `Router` rules → PRODUCT-13 routing rules); dry-run preview using the deep-link confirmation components.
- **Acceptance:** a claude-code-router config with 4 providers and 3 routes imports as 4 providers + 3 routing rules with keys intact.

### PRODUCT-27: First-run onboarding wizard (detect tools, pick provider, choose routing mode)

**Type:** ux · **Priority:** P2 · **Effort:** M · **Confidence:** high · **Persona:** a, e

- **Evidence:** first run shows a text-only `FirstRunNoticeDialog.tsx` (two paragraphs, "Got it"); tool detection already exists (`probe_tool_installations`, `get_tool_versions` in `commands/misc.rs`); manual 1.4 "Quick Start" is a 5-minute manual procedure; README FAQ #3 ("plugin configuration disappeared") and "switch back to official login" are recurring confusions.
- **Problem:** New users meet eight app tabs and a blank list; the most common support questions (restart needed? how to go back to official? why did my settings vanish?) are not answered at the moment they matter.
- **Proposal:** 3-step wizard: detected tools (hide others via `visible_apps`), add first provider per tool (preset search), choose "direct" vs "local routing (recommended for failover/usage)", with the official-provider explanation; re-runnable from Settings; keep the existing dialog as fallback.
- **Acceptance:** a fresh install reaches a working Claude Code provider in ≤4 clicks; `visible_apps` reflects detected tools.

### PRODUCT-28: Accessibility pass (keyboard navigation, ARIA, reduced motion, contrast)

**Type:** ux · **Priority:** P2 · **Effort:** M · **Confidence:** high · **Persona:** a, d

- **Evidence:** 46 `aria-label` occurrences across 166 `.tsx` files; provider card actions "appear on hover and are hidden by default" (manual 1.3) which is hostile to keyboard/screen-reader users; framer-motion transitions on every view switch (`App.tsx` `AnimatePresence`), no `prefers-reduced-motion` handling found; drag-only reordering (`@dnd-kit`) without keyboard alternative documented.
- **Problem:** Keyboard-only and assistive-tech users cannot reach Enable/Edit/Delete reliably; enterprise accessibility checklists fail.
- **Proposal:** Focus-visible action bar on cards, `aria-label`s for icon buttons, roving tabindex in lists, keyboard sort (Alt+↑/↓), `prefers-reduced-motion` guard, axe-core check in vitest for key panels; add an a11y section to CONTRIBUTING.
- **Acceptance:** axe reports 0 serious violations on providers/settings/usage; full switch flow possible with keyboard only.

### PRODUCT-29: Diagnostics bundle ("Report a problem") with redacted logs, settings and tool versions

**Type:** feature · **Priority:** P2 · **Effort:** S · **Confidence:** high · **Persona:** a, f

- **Evidence:** bug template asks for version/OS/app/logs by hand (`.github/ISSUE_TEMPLATE/bug_report.yml`); manual 5.2 "Log Files: attach log files… `~/.cc-switch/logs/`"; data already available via `get_tool_versions`, `probe_tool_installations`, `check_env_conflicts`, `get_proxy_status`, `get_log_config`.
- **Problem:** Maintainers triage hundreds of issues (PR numbers >5500) with inconsistent environment info; users paste logs with keys in them.
- **Proposal:** Settings → About → "Create diagnostics bundle": zip of last N log lines with secret redaction (regex for `sk-`, bearer, keys from DB), settings minus secrets, tool versions, env-conflict summary, proxy/takeover state, DB schema version; copy a markdown summary to clipboard for the issue form; link from the issue template.
- **Acceptance:** bundle contains no provider secrets (test with planted key); issue template references the feature.

### PRODUCT-31: Request-log retention and privacy controls

**Type:** feature · **Priority:** P2 · **Effort:** S · **Confidence:** medium · **Persona:** d, a

- **Evidence:** `proxy_request_logs` stores per-request metadata incl. `session_id`, `error_message` (`schema.rs:197-212`); no prune/cleanup function found in `database/dao/proxy.rs` or `usage_rollup.rs` (grep `prune|cleanup|delete_old|purge` empty) although `usage_daily_rollups` exists; manual 4.1 only offers "Enable Logging" on/off.
- **Problem:** Heavy users accumulate unbounded rows (SQLite growth, slower dashboard queries that the 2 s/5 s pollers in `useProxyStatus.ts:28`, `query/proxy.ts:16` hit constantly); privacy-minded users cannot keep rollups while dropping per-request rows.
- **Proposal:** Retention settings (keep detail N days / max rows; keep rollups forever), scheduled VACUUM, "Delete logs now" with confirmation; show DB size in Settings → Data; document what is stored (no bodies).
- **Acceptance:** with 7-day retention, rows older than 7 days are gone after the nightly job while rollup totals are unchanged.

### PRODUCT-32: API key pool / rotation per provider (multiple keys, rotate on 429/401)

**Type:** feature · **Priority:** P2 · **Effort:** M · **Confidence:** high · **Persona:** a, e

- **Evidence:** a provider holds exactly one key in `settingsConfig`; multi-endpoint exists (`provider_endpoints`, `endpointAutoSelect` meta) but no multi-key; the OAuth centers support multiple *accounts* only for Copilot/Codex (manual 2.1 "Multi-Account Management"); manual 2.4 recommends "Duplicate" to use a different account, which creates separate cards and splits usage stats.
- **Problem:** Relay users with several keys (per-key concurrency/quota) juggle duplicate cards; 429s on one key cannot transparently move to the next.
- **Proposal:** `keys[]` with labels and optional daily cap per key in provider meta; proxy selects round-robin/least-used and rotates on 429/401 before failing over to another provider; non-proxy switch writes the "active key"; usage logs record `key_label`.
- **Acceptance:** two keys with a forced 429 on key A → request succeeds on key B without provider failover; card shows per-key status.

### PRODUCT-33: Capability matrix in docs and in-app ("what works for this app")

**Type:** docs · **Priority:** P2 · **Effort:** S · **Confidence:** high · **Persona:** a, f

- **Evidence:** coverage differs per subsystem (see inventory: takeover 4+1 apps, tray 4, profiles 3, universal 3, prompts 6, skills/MCP 7 (no OpenClaw; no Claude Desktop MCP), stream check 6, terminal launch 1); README claims "Unified MCP & Skills Management… across Claude, Codex, Gemini, Grok Build, OpenCode, and Hermes" while manual 3.3 lists five apps and manual 1.1 still says "seven" tools after Grok Build (`1c0ee0c`, 2026-07-17).
- **Problem:** Users enable a feature for an app that silently does not support it (e.g. Skills on OpenClaw, failover on Hermes) and file bugs.
- **Proposal:** Single generated matrix (`docs/user-manual/*/capabilities.md`) sourced from a TS/Rust capability table (`AppCapabilities` per `AppId`), rendered in Settings → About and used by UI to hide/explain unsupported buttons instead of ad-hoc guards in `App.tsx`.
- **Acceptance:** matrix auto-tested against the guard logic; manual 1.1/3.x reference it.

### PRODUCT-34: Documentation bugs batch (CONTRIBUTING paths/scripts, issue-template apps, port, health claim, zh-only guide)

**Type:** docs · **Priority:** P2 · **Effort:** S · **Confidence:** high · **Persona:** f, a

- **Evidence:** `CONTRIBUTING.md` points to `src/locales/{en,zh,ja}/translation.json` (actual: `src/i18n/locales/{en,ja,zh,zh-TW}.json`) and lists `pnpm lint` (no such script in `package.json`), says "three languages" (four); `.github/ISSUE_TEMPLATE/*.yml` app dropdowns lack Claude Desktop, Grok Build, Hermes; manual 5.2 uses `lsof -i :49152` while default port is 15721 (4.1); manual 4.5 documents periodic health checks that do not exist (PRODUCT-03); `docs/guides/proxy-guide-zh.md` has no en/ja; manual 1.1 says seven tools.
- **Problem:** Contributors edit the wrong files/commands; triage lacks the right app label; users debug the wrong port.
- **Proposal:** Fix the listed items; add a docs CI check that greps manual for app names against `APP_IDS` and for `pnpm` scripts against `package.json`.
- **Acceptance:** all cited discrepancies gone; CI check in place.

### PRODUCT-35: Windows Authenticode signing (SmartScreen) and SBOM/provenance for releases

**Type:** risk · **Priority:** P2 · **Effort:** S · **Confidence:** high · **Persona:** d, a

- **Evidence:** `.github/workflows/release.yml` imports an Apple certificate (line 189) and prepares the Tauri minisign key (133-187) but has no `signtool`/Azure Trusted Signing step; README/manual only mention macOS notarization; imposter-site warnings in every release note make binary provenance a trust issue.
- **Problem:** Windows users see SmartScreen "unknown publisher" (indistinguishable from the imposters), and enterprises cannot verify provenance.
- **Proposal:** Add Azure Trusted Signing (or OV cert) step for MSI/portable, publish SHA256SUMS + SLSA provenance/attestations and a CycloneDX SBOM per release; document verification in manual 1.2.
- **Acceptance:** MSI shows "Verified publisher"; `gh attestation verify` passes on release assets.
- **Verification:** **confirmed** — Fully verified: .github/workflows/release.yml:133-187 prepares the Tauri minisign key and :189-212 imports the Apple .p12, but no `signtool`/Azure Trusted Signing/`attest`/`sbom`/`sha256` step exists anywhere under .github/workflows, workflow permissions are only `contents: write` (release.yml:8-9, 591-592, 640-641; no id-token), and src-tauri/tauri.conf.json:47-51 configures only a WiX template with no certificateThumbprint/signCommand; MSI and Portable zip (release.yml:268, 477-505) are uploaded straight via softprops/action-gh-release@v3 (release.yml:608). Docs mention only macOS signing (docs/user-manual/en/1-getting-started/1.2-installation.md:141-143, README.md:278) while the Windows section (1.2:105-118) is silent on SmartScreen, and imposter warnings are present (docs/release-notes/v3.16.0-en.md:23-33, v3.17.0-en.md:32-34). The only partial mitigation is the Tauri updater's minisign verification (tauri.conf.json:63 pubkey; release.yml:711-716 writes per-platform signatures into latest.json), which protects in-app updates but not first installs. Priority corrected to P2: it is trust/distribution hardening rather than an exploitable condition, and effort is realistically M rather than S because Authenticode/Trusted Signing requires identity validation and a paid cert.

### PRODUCT-36: Deep-link `usageScript` hardening (script preview, explicit opt-in, SSRF review)

**Type:** risk · **Priority:** P2 · **Effort:** S · **Confidence:** medium · **Persona:** d, e

- **Evidence:** deep links accept `usageScript`, `usageEnabled` (default true), `usageApiKey`, `usageBaseUrl` (manual 5.3 provider parameters); scripts execute in `rquickjs` (`usage_script.rs:1-51 ctx.eval`) and the Rust side performs the HTTP request (`send_http_request`, with `url::Host` imports suggesting host checks); import shows a "configuration preview" (manual 5.3).
- **Problem:** A sponsor/tutorial link can ship a script that exfiltrates `{{apiKey}}` to an attacker URL on every auto-refresh; the preview may not make the script's request target obvious.
- **Proposal:** In the import dialog, render the script's `request.url` host and headers prominently, default `usageEnabled=false` for deep-link imports, block private/loopback targets unless the user opts in, and show "this script will send your key to <host>" copy.
- **Acceptance:** a link whose script posts the key to a third-party host requires an explicit extra confirmation naming the host.

### PRODUCT-40: Pricing currency semantics and FX setting

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** medium · **Persona:** e, a

- **Evidence:** manual 4.4 preset table lists DeepSeek/Kimi/MiniMax/GLM prices in ¥ while the Hero/summary is "Estimated Cost (USD)" and `check_provider_limits` sums `total_cost_usd`; v3.13.0 notes say it "corrects CNY → USD pricing for several models" and 3.17.0 Hy3 pricing was "converted from the launch-day list price of CNY"; `costMultiplier` is a string multiplier only (`src/types.ts:190`).
- **Problem:** Users cannot tell whether a ¥-priced entry is summed as dollars; mixed-currency presets make totals and budgets wrong by ~7x.
- **Proposal:** Add `currency` to `model_pricing` rows and a global FX rate setting (manual or fetched), normalize everything to the display currency, show currency badge in `PricingConfigPanel`; audit seeds.
- **Acceptance:** a ¥1/M model with FX 7.2 shows $0.139/M and totals reconcile; docs table states currency per row.

### PRODUCT-43: Adjacent tool support wave 1 (Qwen Code, Kimi CLI, Goose, Aider, Zed) via adapter trait

**Type:** feature · **Priority:** P2 · **Effort:** XL · **Confidence:** medium · **Persona:** a, e, f

- **Evidence:** adding Grok Build touched 112 files / +4,530 lines (`git show --stat 1c0ee0c`: new `grok_config.rs`, session provider, form, usage/pricing, tray, settings, MCP, skills, i18n, tests); no adapter trait — each app is matched explicitly across `services/proxy.rs`, `tray.rs`, `prompt_files.rs`, `services/mcp.rs`, `services/skill.rs`, `settings.rs` (8 `*_config_dir` fields); `AppType::all()` exists.
- **Problem:** Each new tool costs weeks and increases the complexity ceiling (eight apps, 14 views); the best-fit candidates are file-config tools: Qwen Code (`~/.qwen/settings.json` + `.env`, a Gemini CLI fork — near-copy of `gemini_config.rs`), Kimi CLI (`~/.kimi` config), Goose (`~/.config/goose/config.yaml` provider/model + env keys), Aider (`~/.aider.conf.yml` + env, model aliases), Zed (`settings.json` `language_models` with OpenAI/Anthropic-compatible providers), Droid/Factory (`~/.factory/config.json` custom models), Continue (`config.yaml`); poor fit: Cursor/Windsurf/JetBrains AI (IDE-internal stores, no file API), Copilot CLI (GitHub OAuth only), Warp (already a terminal target), Amp (OAuth/`settings.json` with limited provider surface).
- **Proposal:** First land an `AppAdapter` trait (config dir, read/write live provider, MCP/prompt/skills paths, takeover apply/restore, session source, capability flags) and port Gemini + Grok to it as proof; then ship Qwen Code and Goose as the first adapter-based apps (both reuse Gemini/OpenAI-compatible bridges), followed by Aider/Zed; presets for partners come from the catalog (PRODUCT-07).
- **Acceptance:** a new file-config app is added in ≤25 files/≤1.5k lines; capability matrix (PRODUCT-33) updates automatically.

### PRODUCT-44: Backend modularization: split `commands/misc.rs`, introduce `cc-switch-core` crate

**Type:** refactor · **Priority:** P2 · **Effort:** L · **Confidence:** high · **Persona:** f

- **Evidence:** `commands/misc.rs` is 5,414 lines (tool lifecycle code), `codex_config.rs` 3,497, `services/proxy.rs` 7,215 lines, `tray.rs` 1,532; 267 commands registered flat in `lib.rs`; frontend `src/lib/api/vscode.ts` still hosts generic endpoint/import-export commands (naming debt); README FAQ #3 "Shared Config Snippet" semantics live in both `config.rs` and front-end TOML/JSON merge paths (v3.17.0 moved TOML merge to backend).
- **Problem:** Reviewing a proxy or tool-management change means reading multi-thousand-line files; PRODUCT-10/11 (daemon/CLI) need a UI-free core; AI-assisted contributions (CONTRIBUTING "AI-Assisted Contributions") amplify sprawl.
- **Proposal:** Workspace with `core` (db, services, proxy, adapters), `app` (Tauri commands/tray), `cli`, `proxyd`; split `misc.rs` into `tools/{probe,install,upgrade}.rs`; command registration via per-domain `generate_handler!` lists; add an ADR folder (`docs/adr/`) and a CLAUDE.md/AGENTS.md describing layering for AI tools.
- **Acceptance:** `cargo build -p cc-switch-core` has no Tauri dependency; no file >1,500 lines in `commands/`.

### PRODUCT-45: End-to-end tests for switch/takeover/failover flows

**Type:** refactor · **Priority:** P2 · **Effort:** M · **Confidence:** high · **Persona:** f

- **Evidence:** CI runs vitest (68 test files, MSW-mocked IPC) and `cargo test` (12 integration files) only (`.github/workflows/ci.yml`); no tauri-driver/WebdriverIO/Playwright config; regressions like "Takeover-Residue Recovery", "Proxy Placeholder Backup / Restore Loop" (v3.16.2/3.16.3), "Deleted Codex MCP Servers No Longer Come Back" (3.17.0) are integration-level.
- **Problem:** The highest-risk paths (live-file rewrite + proxy + restore) are only covered by unit tests with mocks.
- **Proposal:** Rust integration harness with a temp HOME, fake upstream (axum mock returning canned SSE), and scripted scenarios: add→switch→takeover→kill upstream→failover→restore→verify files byte-exact; run on the existing Windows/macOS backend matrix (`f6e37ed`); add a small `tauri-driver` smoke on Linux for the UI switch.
- **Acceptance:** scenario suite in CI; each "residue"/"blanking" bug class has a scenario.

### PRODUCT-51: Stream check coverage gap for Gemini and Grok Build

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** medium · **Persona:** b

- **Evidence:** `services/stream_check.rs` matches Claude/ClaudeDesktop/Codex/OpenCode/OpenClaw/Hermes (no `AppType::Gemini`/`GrokBuild`), while manual 4.5 says coverage includes Gemini and a "Gemini Model" test setting exists; Grok Build was added after 3.17.0.
- **Problem:** Gemini/Grok providers either cannot be tested or hit an unexpected path; failover queues for Gemini lose the manual probe.
- **Proposal:** Verify by running the check on a Gemini provider; implement the Gemini native `generateContent` probe (bridge already exists in `proxy/providers/gemini.rs`) and Grok via its OpenAI-compatible path; update docs.
- **Acceptance:** "Test" works on Gemini and Grok cards; manual lists accurate coverage.

### PRODUCT-54: Sponsor preset governance policy (criteria, review cadence, removal, disclosure) as a public document

**Type:** risk · **Priority:** P2 · **Effort:** S · **Confidence:** high · **Persona:** e, d

- **Evidence:** 35/71 Claude presets are paid partners; README sponsor copy makes performance/"no dilution" claims the project cannot verify; removals/demotions are undocumented beyond changelog lines (v3.16.0/3.16.3); imposter sites exploit the brand (release-note warnings); sponsor inquiries go to a personal email (README "Want to appear here?").
- **Problem:** Users and enterprises cannot assess why a relay is "recommended"; a bad actor sponsor could harm users and the project's trust.
- **Proposal:** `docs/SPONSORS.md` with listing criteria (company registration, refund policy, status page, abuse contact), what sponsorship buys (placement only, never routing defaults), review cadence, removal triggers, affiliate disclosure (PRODUCT-08), and a public preset-change log generated from the catalog (PRODUCT-07).
- **Acceptance:** policy published and linked from README/App About; each partner preset references a compliance date.
- **Verification:** **confirmed** — Counts check out: src/config/claudeProviderPresets.ts has 70 preset entries (by `category:` lines; the reviewer's 71 counted `name:` lines) of which 35 carry `isPartner: true` (codexProviderPresets.ts is 34/61), and `primePartner` pins sponsors to the top of the picker (src/components/providers/forms/ProviderPresetSelector.tsx:99-103). README.md:60 and :74 carry unverifiable claims ('100% sourced from official channels - no dilution, no fakes', '10x compensation guarantee'), README.md:23 routes sponsor inquiries to mailto:farion1231@gmail.com, affiliate/promo parameters appear repeatedly (aff=cc-switch x5, utm_source x7, 'promo code' x7) with no 'affiliate' or 'disclosure' wording anywhere in README.md, and in-app promo copy ships in src/i18n/locales/en.json:1018-1030. Removals/demotions exist only as changelog lines (CHANGELOG.md:214-215 MiniMax demoted and LemonData removed in 3.16.3; :416-417 LionCC/AICoding removed in 3.16.0), and no SPONSORS/governance/policy document exists (find over the repo and docs/user-manual/en/README.md return nothing; SECURITY.md covers only vulnerability reporting). I found no evidence sponsorship alters routing defaults, only placement, which matches the proposal. A governance document is a trust/transparency deliverable with no exploitable condition, so P2 rather than P1.

### PRODUCT-55: Information architecture for eight apps (sidebar + app-scoped feature rail, progressive disclosure)

**Type:** ux · **Priority:** P2 · **Effort:** M · **Confidence:** medium · **Persona:** a, e

- **Evidence:** 14 views in `App.tsx` driven by a top toolbar with per-app conditional buttons and an auto-compacting toolbar (`useAutoCompact`), settings with 6 tabs + 6 sub-tabs, 3 hidden-by-default toggles (`enableLocalProxy`, `enableFailoverToggle`, `showProfileSwitcher` in `settings.rs:365-379`), OpenClaw-only views (env/tools/agents/workspace), Hermes-only memory; FAQ "If you rarely use a specific CLI tool, you can hide it in Settings".
- **Problem:** Each new app/feature adds another conditional button; discoverability of proxy/failover/projects depends on finding Settings toggles (the 2.6 manual spends a section explaining how to reveal the routing toggle).
- **Proposal:** Left rail (apps) + top tabs (Providers / Extensions / Routing / Usage / Sessions) with per-app availability from the capability table (PRODUCT-33); surface routing/failover/projects as first-class tabs instead of hidden toggles; keep compact mode for small windows.
- **Acceptance:** no feature requires a Settings toggle to become visible; usability test on 5 new users finds proxy/failover unaided.

### PRODUCT-61: THEME — "Proxy as a product" (daemon + CLI + proactive health + notifications + smart routing + web dashboard)

**Type:** feature · **Priority:** P2 · **Effort:** XL · **Confidence:** medium · **Persona:** b, a, c

- **Evidence:** PRODUCT-03/06/10/11/13/30/57; changelog shows the proxy is where development and user pain concentrate (every release since 3.12 lists 10-30 proxy items).
- **Problem:** The proxy is the product's moat but is still an embedded feature of a window-bound app.
- **Proposal:** Sequence: health probes (03) → notifications (06) → core crate split (44) → daemon (10) + CLI (11) → routing rules (13) → dashboard (30) + trace (57). Ship behind "Advanced routing" so the simple path stays simple.
- **Acceptance:** a headless Linux box runs failover for Claude Code via `ccs` with no GUI; GUI users see no regression.
- **Verification:** **partially-confirmed** — The theme's building blocks are genuinely missing where I could check: src-tauri/src/proxy/health.rs:1-7 is a placeholder (`HealthChecker` with no logic) so proactive probes do not exist, no notification plugin is in src-tauri/Cargo.toml, there is no CLI/daemon (see PRODUCT-10/11), and proxy/server.rs:291-300 serves only `/health`, `/status` and API routes with no dashboard. But the evidence is overstated on two points: counting proxy/takeover/failover/routing lines per CHANGELOG.md section gives 22 (3.17.0), 36 (3.15.0), 14-17 (3.13.0/3.14.0/3.16.0/3.16.2/3.16.3) yet only 1-8 for 3.12.1-3.12.3, 3.14.1, 3.16.4 and 3.16.5, so 'every release since 3.12 lists 10-30 proxy items' is false for patch releases; and the proxy is not strictly 'window-bound' because the app is tray-resident with silent startup and auto-launch (settings.rs:359-362, 497-509) and restores takeover on boot (lib.rs:1068, 1770). A circuit breaker and reactive failover already exist (proxy/circuit_breaker.rs; docs/user-manual/en/4-proxy/4.3-failover.md:101-135). The sequencing cites sibling findings (03/06/13/30/44/57) I could not verify, and an XL roadmap theme is not a defect, so P2 with medium confidence.

### PRODUCT-64: THEME — "Platform SDK" (adapter trait, plugin surface for adapters/usage templates, new-app wave)

**Type:** refactor · **Priority:** P2 · **Effort:** XL · **Confidence:** medium · **Persona:** f, e, a

- **Evidence:** PRODUCT-43/44/33; usage-script templates and coding-plan providers are hardcoded TS (`UsageScriptModal.tsx`, `codingPlanProviders.ts`); skills already have a repo/registry model that could be the template for "usage template" and "adapter" packages.
- **Problem:** Every relay-specific quota endpoint and every new tool lands as core code reviewed by one maintainer.
- **Proposal:** Adapter trait + capability table first; then a signed "extension" package format (JSON manifest + JS usage template + optional preset bundle) installable from a repo like skills, with sandboxed execution (rquickjs already) and policy control; publish an authoring guide.
- **Acceptance:** a community member ships a quota template for a new relay without a core PR; wave-1 apps land via adapters.

### PRODUCT-30: Proxy web dashboard (read-only status/logs served by the proxy)

**Type:** feature · **Priority:** P3 · **Effort:** M · **Confidence:** high · **Persona:** b, c

- **Evidence:** proxy exposes only `/health` and `/status` JSON (`proxy/server.rs:294-295`); all observability lives in the Tauri window (Usage tab); PRODUCT-10 makes a window-less deployment possible.
- **Problem:** Headless/daemon or remote-SSH users (and teammates on a shared LAN gateway with PRODUCT-02 auth) have no way to see failover state or spend.
- **Proposal:** `/dashboard` static page (same React build, slim bundle) + `/api/*` read-only endpoints behind the listener token: queue/breaker state, last 100 requests, today's cost; optional "switch provider" POST when policy allows.
- **Acceptance:** `http://127.0.0.1:15721/dashboard` renders status and logs without the desktop window.

### PRODUCT-37: Tighten CSP and asset protocol scope

**Type:** risk · **Priority:** P3 · **Effort:** S · **Confidence:** high · **Persona:** d

- **Evidence:** `src-tauri/tauri.conf.json:29` `connect-src 'self' ipc: http://ipc.localhost https: http:` and `img-src … https: http:`; CHANGELOG 3.2.0 note "`connect-src` CSP remains permissive for compatibility; can be tightened later".
- **Problem:** Any XSS in the webview (e.g. via provider notes/markdown preview) can call arbitrary hosts with webview privileges.
- **Proposal:** Move remaining direct fetches (skills.sh search, models.dev, GitHub) behind Tauri commands (most already are) and restrict `connect-src` to `ipc:`; keep `img-src https:` for provider icons or bundle them.
- **Acceptance:** app functions with `connect-src 'self' ipc: http://ipc.localhost`.

### PRODUCT-38: Linux polish: in-app GDK backend setting, tray fallback detection, official Flatpak

**Type:** ux · **Priority:** P3 · **Effort:** M · **Confidence:** high · **Persona:** a

- **Evidence:** README FAQ + manual 5.2 describe the `CC_SWITCH_GDK_BACKEND` env escape hatch for Wayland+NVIDIA click-dead/black-screen; `linux_fix.rs`; manual 5.2 "Linux: System tray support may need… libappindicator"; `flatpak/README.md` "Not included in official releases"; v3.15.0 fixed a theme-selection segfault (#2502).
- **Problem:** Env vars in `.desktop` files are a poor UX for a GUI app; tray-less desktops (GNOME without extension) silently lose the main quick-switch feature.
- **Proposal:** Settings → General (Linux): backend selector that writes a launcher wrapper/desktop override; detect missing StatusNotifier host and show a banner with a dock/launcher alternative; publish Flatpak to Flathub from CI (manifest exists).
- **Acceptance:** Wayland users can fix clicks from Settings; Flathub listing live.

### PRODUCT-39: Updater channels (stable/beta) and in-app release notes

**Type:** feature · **Priority:** P3 · **Effort:** S · **Confidence:** high · **Persona:** a, f

- **Evidence:** single updater endpoint (`tauri.conf.json` plugins.updater.endpoints = latest.json); no channel/beta strings in `AboutSection.tsx`/`commands/settings.rs`; release notes are 3-locale markdown in `docs/release-notes/` not shown in-app; v3.16.3 fixed "In-App Update No Longer Hangs on Restart".
- **Problem:** Proxy-bridge changes land weekly; a beta ring would catch regressions (PRODUCT-04) before 100k users hit them; users never read upgrade notes (e.g. 3.17.0 "Kimi For Coding Providers Need a Preset Re-Apply").
- **Proposal:** `beta.json` endpoint + Settings toggle; render the release note "Upgrade Notes" section in the UpdateBadge dialog after update.
- **Acceptance:** beta channel users receive prerelease builds; upgrade notes shown once post-update.

### PRODUCT-41: Provider tags, groups and archive state

**Type:** ux · **Priority:** P3 · **Effort:** S · **Confidence:** high · **Persona:** a, e

- **Evidence:** `Provider` has `notes`, `sortIndex`, `category`, `icon` only (`src/types.ts:11-31`); search covers name/notes/URL (manual 1.3); users keep "personal/work/expired" in free-text notes (manual 2.1 "Notes… Expiration date").
- **Problem:** Lists with 15+ relay cards per app become unmanageable; expired subscriptions linger as live-switchable cards.
- **Proposal:** `tags[]` + `archived` flag in meta; tag filter chips in `ProviderList`; archived cards hidden by default and excluded from tray/failover; structured `expiresAt` with 7-day reminder (uses PRODUCT-06).
- **Acceptance:** tag filter works across apps; archived provider cannot be selected from tray.

### PRODUCT-42: Claude Code subagents & hooks management (replace the `AgentsPanel` stub)

**Type:** feature · **Priority:** P3 · **Effort:** M · **Confidence:** high · **Persona:** b, c

- **Evidence:** `src/components/agents/AgentsPanel.tsx:16` "The Agents management feature is currently under development"; view `"agents"` exists in `App.tsx` but has no entry button; 3.17.0 added `CLAUDE_CODE_SUBAGENT_MODEL` field; common config "carries hooks you added" (v3.16.5) but there is no hooks editor; Claude Code reads `~/.claude/agents/*.md` and `settings.json` `hooks`/`permissions`.
- **Problem:** Users who manage CLAUDE.md, MCP and skills in CC Switch still hand-edit agents and hooks, which the backfill/common-config machinery then has to protect.
- **Proposal:** Agents panel listing `~/.claude/agents` (and project-level) with the Markdown editor, model/tool pickers; Hooks/permissions editor with validation and deep-link share; include both in Projects snapshots.
- **Acceptance:** creating an agent writes a valid frontmatter file Claude Code loads; hooks edits survive provider switches.

### PRODUCT-46: Localization expansion and translation tooling (de UI, key-coverage lint, community workflow)

**Type:** feature · **Priority:** P3 · **Effort:** M · **Confidence:** high · **Persona:** a, f

- **Evidence:** UI locales zh/zh-TW/en/ja (`src/i18n/index.ts:9`), each ~2,975 lines; `README_DE.md` exists (v3.16.0) but no `de.json`; `7e73a1f` "add missing proxyReasonAnthropicMessages key across locales" shows keys drift; CONTRIBUTING asks contributors to update all locale files by hand.
- **Problem:** German/Korean/Spanish users (star growth is global per the top-100 badge) get a zh/en UI; missing keys surface as raw ids.
- **Proposal:** vitest key-parity check (every key in `en.json` exists in all locales), Crowdin/Weblate project with `en.json` as source, add `de` and `ko`, allow partial locales with English fallback, tray strings moved from `tray.rs:97-135` into the same JSON.
- **Acceptance:** CI fails on missing keys; `de` selectable in Settings.

### PRODUCT-47: Opt-in, privacy-preserving crash reporting (no usage telemetry)

**Type:** feature · **Priority:** P3 · **Effort:** S · **Confidence:** medium · **Persona:** f, d

- **Evidence:** no telemetry/sentry/posthog in code (grep); `panic_hook.rs` exists; bug reports rely on user-pasted logs; v3.15.0 fixed a Linux segfault (#2502) and multiple "panic-prone proxy paths" (v3.16.0).
- **Problem:** Panics on user machines are invisible unless reported manually; the project has stated privacy values (keys never uploaded) that a generic telemetry SDK would undermine.
- **Proposal:** Off-by-default "send anonymous crash reports" toggle that uploads only panic message, stack symbols, OS/arch and app version (no paths/keys) to a self-hosted endpoint; show the exact payload before first send; document in SECURITY.md.
- **Acceptance:** payload contains no PII (test); toggle persisted; disabled → zero network calls.

### PRODUCT-48: Session export (Markdown/JSON) and session analytics

**Type:** feature · **Priority:** P3 · **Effort:** S · **Confidence:** high · **Persona:** a, c

- **Evidence:** session manager supports browse/search/resume/delete/batch delete (manual 3.4; `commands/session_manager.rs` 5 commands); no export/download (grep in `sessions/*.tsx` empty); `session-manager.md` PRD scoped v1 to "copy/resume".
- **Problem:** Users cannot archive a conversation to a PR description/wiki or hand a transcript to a teammate; deleting to save space loses it forever.
- **Proposal:** "Export session" (Markdown with role headers, JSON raw) and "Export all filtered" zip; per-provider counts/time charts in the toolbar (reuse recharts); pairs with PRODUCT-14 cost badges.
- **Acceptance:** exported Markdown round-trips tool messages and timestamps; batch export of 100 sessions completes <5 s.

### PRODUCT-49: Model catalog sync across apps (one upstream model list → Codex catalog / OpenClaw models / OpenCode models / Claude mappings)

**Type:** feature · **Priority:** P3 · **Effort:** M · **Confidence:** medium · **Persona:** a, e

- **Evidence:** model discovery exists per form (`fetch_models_for_config`, manual 2.1 "Auto-Fetch Models"); Codex needs a projected `~/.codex/cc-switch-model-catalog.json` (v3.16.x), OpenClaw has `set_openclaw_model_catalog`, OpenCode per-model limits (3.17.0), Claude needs role mappings — each maintained separately; v3.16.5 "Existing Native Codex Providers Need a One-Time Re-Save" and 3.17.0 "Kimi For Coding Providers Need a Preset Re-Apply" show catalog drift pain.
- **Problem:** When a relay adds `gpt-5.6-luna`, users re-fetch and re-map in up to six forms.
- **Proposal:** Store a per-universal-provider model catalog (id, display, context, modalities, pricing link) refreshed on demand; project it into each app's format on sync; "model changed upstream" notice using PRODUCT-06.
- **Acceptance:** refreshing a universal provider's catalog updates Codex catalog JSON and OpenClaw models in one action.

### PRODUCT-50: Usage data export (CSV/JSON) and scheduled monthly report

**Type:** feature · **Priority:** P3 · **Effort:** S · **Confidence:** high · **Persona:** a, c

- **Evidence:** Usage tab shows tables/charts only (manual 4.4); no CSV/export in `src/components/usage/*.tsx` (grep); "Refer to the provider's invoice for authoritative numbers" (4.4 FAQ) implies reconciliation needs.
- **Problem:** Expense reports and reconciliation with relay invoices are manual screenshots.
- **Proposal:** Export current filter as CSV/JSON (request logs, provider/model/project stats); optional monthly summary file written to a chosen folder (and notification).
- **Acceptance:** CSV columns match the request-log table; totals equal the Hero.

### PRODUCT-52: Claude Desktop on Linux (3P profile writing)

**Type:** feature · **Priority:** P3 · **Effort:** M · **Confidence:** medium · **Persona:** a

- **Evidence:** manual 2.6 "Supported systems: macOS, Windows; Not supported yet: Writing Claude Desktop 3P configuration on Linux"; `claude_desktop_config.rs` (2,216 lines) resolves `~/Library/Application Support` / `%LOCALAPPDATA%` only.
- **Problem:** Linux users with community Claude Desktop builds get a status "Current platform is not supported".
- **Proposal:** Add `$XDG_CONFIG_HOME/Claude` path resolution behind a "custom Claude Desktop dir" setting (same pattern as `*_config_dir` overrides) with the same profile writer; keep the warning when the directory is absent.
- **Acceptance:** Linux users with a Claude Desktop config dir can enable a 3P provider.

### PRODUCT-53: Skills for OpenClaw and prompts for Hermes/OpenClaw

**Type:** feature · **Priority:** P3 · **Effort:** S · **Confidence:** medium · **Persona:** a

- **Evidence:** `appConfig.tsx:30-37` excludes OpenClaw from `SKILLS_APP_IDS` ("it doesn't support Skills") yet manual 5.1 shows `~/.openclaw/skills/`; `prompt_files.rs:12` has no Hermes/OpenClaw targets though Hermes has `SOUL.md` and OpenClaw `AGENTS.md`/`SOUL.md` (editable only via Workspace panel).
- **Problem:** OpenClaw users cannot install skills from the shared SSOT; Hermes users cannot use prompt presets/backfill.
- **Proposal:** Confirm OpenClaw's skills directory semantics, add it to `SKILLS_APP_IDS` + `services/skill.rs` targets; map Hermes prompts to `SOUL.md` and OpenClaw to workspace `AGENTS.md` with the existing backfill protection.
- **Acceptance:** a skill installed "for all apps" appears in `~/.openclaw/skills`; Hermes shows the Prompts button.

### PRODUCT-56: Quota/subscription lifecycle: reset countdown everywhere, expiry reminders, auto-refresh when inactive

**Type:** feature · **Priority:** P3 · **Effort:** S · **Confidence:** high · **Persona:** a, e

- **Evidence:** quota footers exist for official/OAuth/coding-plan providers (manual 2.5) but "Background queries only trigger when the provider is Currently Active"; expiry dates live in free-text notes (manual 2.1); tray shows cached usage summaries only for current providers.
- **Problem:** A backup subscription's quota is stale exactly when failover needs it; monthly plans expire unnoticed.
- **Proposal:** Optional low-frequency refresh for failover-queue members, structured `expiresAt`/`renewsAt` with 3-day reminder (PRODUCT-06), and a "quota ≥90%" pre-emptive failover option.
- **Acceptance:** queue members show quota ≤1 h stale; reminder fires once.

### PRODUCT-57: `/status` and request-detail: expose routing decisions and rectifier actions

**Type:** ux · **Priority:** P3 · **Effort:** S · **Confidence:** high · **Persona:** b

- **Evidence:** rectifiers (thinking, thinking-budget, media sanitizer, image fallback, cache injector in `proxy/*.rs`) silently modify requests; request detail shows params/summary/errors (manual 4.4) and a pricing-model audit (v3.16.3) but not which rectifier/bridge/retry ran; log codes exist (`proxy/log_codes.rs`).
- **Problem:** When a response is wrong, users cannot tell whether failover, a rectifier, or format conversion caused it; maintainers ask for logs.
- **Proposal:** Persist a compact `trace` array per request (bridge used, rectifiers applied, retries/failover hops, cache breakpoints injected) and render it in `RequestDetailPanel` and `/status` (last N).
- **Acceptance:** a failed-over request shows hop list; a request with stripped images shows the rectifier entry.

### PRODUCT-58: Provider terminal launch for all apps and "open with provider env" without writing live config

**Type:** feature · **Priority:** P3 · **Effort:** S · **Confidence:** high · **Persona:** b

- **Evidence:** `App.tsx` `onOpenTerminal={activeApp === "claude" ? handleOpenTerminal : undefined}`; `open_provider_terminal` in `commands/misc.rs`; terminal picker supports many terminals (manual 1.5).
- **Problem:** Codex/Gemini/OpenCode users cannot launch an isolated session with provider B while provider A stays active; the only way is a global switch.
- **Proposal:** Generalize to all apps using per-app env injection (`OPENAI_API_KEY`+`OPENAI_BASE_URL` / `GEMINI_API_KEY` / app config-dir override via `CODEX_HOME`-style vars where supported) so the live config is untouched; show which env is injected.
- **Acceptance:** "Open terminal" on a Codex card starts `codex` using that provider without changing `config.toml`.

### PRODUCT-59: Prompt library sharing: per-project prompt presets and multi-app prompts

**Type:** feature · **Priority:** P3 · **Effort:** M · **Confidence:** high · **Persona:** c, a

- **Evidence:** manual 3.2 "Prompts are managed separately per app… To use the same prompt across multiple apps, you need to create them separately"; targets are global files only (`~/.claude/CLAUDE.md`, `~/.codex/AGENTS.md`, `prompt_files.rs:12`); sharing via deep link only.
- **Problem:** Teams keep one canonical CLAUDE.md/AGENTS.md and copy it by hand into each app and repo.
- **Proposal:** Prompt presets with multi-app targets (one preset → CLAUDE.md + AGENTS.md + GEMINI.md), optional project-level target (`<repo>/CLAUDE.md`) chosen from Projects bindings (PRODUCT-12), import from a Git URL with "update available" like skills.
- **Acceptance:** one preset activates across three apps; project-level activation writes to the repo file with backfill protection.

### PRODUCT-60: Dead/stub code cleanup: unreachable `agents` view, `api/vscode.ts` naming, unused `HealthChecker`

**Type:** refactor · **Priority:** P3 · **Effort:** S · **Confidence:** high · **Persona:** f

- **Evidence:** `View` includes `"agents"` and renders `AgentsPanel` stub but no `setCurrentView("agents")` exists (grep); `src/lib/api/vscode.ts` holds generic endpoint/import-export commands long after VS Code integration was removed (CHANGELOG 3.4.0); `proxy/health.rs` 7-line stub.
- **Problem:** Misleading entry points for new contributors; stale naming drives wrong assumptions.
- **Proposal:** Remove or finish (PRODUCT-42 / PRODUCT-03); rename `vscode.ts` → `endpoints.ts` + `importExport.ts`.
- **Acceptance:** no unreachable views; no stub modules.
