# CC Switch: Next-Phase Plan (v3.18 → v4.0)

## Plan scoring

| Plan | Feasibility (small OSS team) | Risk reduction | User value | Long-term maintainability | Total |
|---|---|---|---|---|---|
| **A: Stability and risk first** | 7 | **10** | 5 | 9 | **31** |
| B: Architecture first | 5 | 8 | 4 | **10** | 27 |
| C: Value every release | 6 | 7 | **9** | 8 | 30 |

- **A is the spine.** It orders the work by where regressions actually come from: bridges, cache and cost accounting, takeover. It pins behavior before moving code. Its exit criteria can be checked mechanically.
- **Taken from C:**
  - Each phase ships as a named release with user-visible items.
  - "Capture and export bug report", which feeds the fixture corpus.
  - Canonical `UsageIR` and the `StopReason` table move earlier, ahead of the full codec IR.
  - The usage parity badge, the capability matrix, takeover for additive apps, OpenClaw MCP, budget alerts and pre-content mid-stream failover.
  - Fix-commit rate as a KPI.
- **Taken from B:**
  - One release per phase (v3.18 to v4.0).
  - The 100% `IpcError` target.
  - A CI check on command-body length.
  - A tauri-driver smoke test on release bundles.
  - The "add a dummy 9th app" acceptance test.
  - Per-domain `collect_commands!` registration.

---

## 1. Executive summary

- **Problem.** Recent git history is mostly fix commits in protocol bridges (6d316c0, 27ce0a5, 650905a, 613fef7, f991726) and in cache and cost accounting. The 1-hour TTL write accounting shipped in 13e7c1f and was reverted in 6eb217b. Adding Grok Build (1c0ee0c) touched 112 files. The cause is structural: each of 5 bridges is written 3 times, about 74 `match app` sites are spread across the code, and the cache-inclusive app rule is copied in 5 places that already disagree.
- **Phase 0 builds the safety net first.** It adds golden bridge fixtures with chunk-split fuzzing, a cost golden table, a migration-equivalence harness, config-writer round-trip snapshots, an in-process proxy E2E harness, and CI gates that match what CONTRIBUTING.md claims.
- **Phase 1 is a "trust" release with no structural change.** It removes free-form shell input from `launch_session_terminal`, stops plaintext key temp files, and fixes unredacted deep-link logging, non-atomic rc-file writes, spend limits that fail open, the Grok Build cost backfill, built-in prices that come back after deletion, and sync overwriting `session_log_sync`.
- **Phase 2 adds typed vocabulary next to the existing code.** That means `WireFormat`/`RoutePlan` types, an `AppDescriptor` registry, `IpcError` codes, and generated bindings via a tauri-specta spike with ts-rs as the fallback. After this phase the compiler checks dispatch before any code moves.
- **Phases 3 to 5 strangle the god modules one seam at a time,** each behind green fixtures:
  - Proxy request path (Phase 3).
  - Per-app adapters, takeover and persistence (Phase 4).
  - Canonical protocol IR and codecs, one bridge per release (Phase 5).
  - The frontend decomposition runs as a parallel track once bindings land.
- **Every phase ships features users can see:** bug-report capture, a per-request trace inspector, proxy takeover for OpenCode/OpenClaw/Hermes, OpenClaw MCP, a capability matrix, budget alerts, a retention panel, cross-platform session resume, and pre-content mid-stream failover.
- **No long-lived branch.** Every structural PR keeps the old path callable until parity is shown, ships with fixtures, and lands on main. Bug fixes made during the migration must add a fixture first.
- **Size:** about 30 to 38 engineer-weeks. **Minimum cut:** if capacity runs short, Phases 0 to 2 plus the Phase 4 registry and takeover work deliver most of the risk reduction and most of the "cheap new app" payoff.

---

## 2. Target architecture

### 2.1 Backend (`src-tauri/src`)

```
apps/
  registry.rs        static APPS: &[AppDescriptor]  (slug, aliases, label, SyncMode, AppCaps bitflags,
                     proxy_route, input_token_semantics, default_visible)
  adapter.rs         trait AppAdapter + optional McpProjector, ProxyTakeover, SessionSource
  <app>/             claude, claude_desktop, codex, gemini, grok, opencode, openclaw, hermes
    config.rs        (moved from *_config.rs)   mcp.rs   takeover.rs
proxy/
  endpoints.rs       ClientEndpoint table -> server.rs routes -> one handle_proxied
  protocol/format.rs WireFormat, ClaudeApiFormat, AppRef, BreakerKey
  protocol/ir.rs     ChatRequest, Part, StreamEvent, StopReason, UsageIR      (Phase 5; UsageIR Phase 3)
  routing/plan.rs    RoutePlan (client, upstream, url, AuthStrategy, VendorQuirks, side stores)
  pipeline/          failover.rs (AttemptRecorder), recovery.rs (RecoveryStrategy),
                     request_builder.rs (BodyStages), headers.rs (HeaderPolicy), transport.rs, validate.rs
  stream/            sse_decoder.rs, events.rs, taps.rs (UsageTap/TimingTap/CaptureTap), aggregate.rs
  codec/{anthropic,openai_chat,openai_responses,gemini}/{request,response,stream}.rs   (Phase 5)
  errors/render.rs   one renderer per client WireFormat
  runtime config:    ArcSwap<RuntimeConfigSnapshot>  (no DB reads on the request path)
services/
  proxy/{mod,lifecycle,takeover,backup,hot_switch,config}.rs   (replaces 7.2k-line services/proxy.rs)
  mcp.rs (generic loops)   usage_ingest.rs   usage_query.rs   cli_tools/*
pricing/  catalog.json, resolver.rs, engine.rs   (the only cost function)
database/
  migrations/        ordered MIGRATIONS, frozen V1 baseline, schema_migrations audit
  repo/{usage,pricing,session_cursor,mcp,skills}.rs   (the only SQL)
  one WAL writer + read pool, busy_timeout, RetentionPolicy + Maintenance::run
commands/<domain>/   providers, proxy, integrations, assets, data, app
                     thin adapters, Result<T, IpcError>, collect_commands!
permissions/<domain>.toml + privileged set
app_shell/           plugins, logging, bootstrap (StartupStep pipeline), run_events (single deep-link router), tray/{model,menu,events}
platform/terminal/{macos,linux,windows}.rs   (merged with session_manager/terminal)
```

### 2.2 Proxy pipeline (target)

```
 client ──HTTP──▶ server.rs (routes generated from endpoints.rs ClientEndpoint table)
                    │
                    ▼
            handle_proxied(&ClientEndpoint)
            ├─ BodyDecoding (plain | codex zstd/gzip)
            ├─ RequestContext  ◀── ArcSwap<RuntimeConfigSnapshot>   (no DB)
            └─ ProviderRouter::select_providers (queue ∩ breaker; BreakerKey)
                    │
                    ▼
   ┌──────────── pipeline::failover (AttemptRecorder owns status/breaker/health/try_switch) ─────────┐
   │ for provider in candidates:                                                                     │
   │   plan = routing::plan(provider, endpoint, body)  ──▶ RoutePlan{client, upstream, url, auth, …} │
   │   body = request_builder [ModelMapping → VendorNormalize → Encode(codec) → LocalOverrides →     │
   │                           Canonicalize]                                                         │
   │   headers = HeaderPolicy::from(&plan)                                                           │
   │   resp = transport::send ─▶ validate (2xx semantics, stream priming)                            │
   │   on error: RecoveryStrategy {MediaDowngrade | ThinkingSignature | ThinkingBudget} (≤1 each)    │
   │            else Retryable → next provider | NonRetryable/ClientAbort → render                   │
   └───────────────────────────── ForwardResult { resp, plan } ──────────────────────────────────────┘
                    │
                    ▼
      stream::SseDecoder ─▶ StreamDecoder(plan.upstream) ─▶ StreamEvent* ─┬─▶ StreamEncoder(plan.client) ─▶ client
                                                                          ├─▶ UsageTap  ─▶ UsageIR ─▶ pricing::engine ─▶ UsageRepo
                                                                          ├─▶ TimingTap (TTFB, first token)
                                                                          └─▶ CaptureTap (opt-in, redacted fixture export)
      non-stream: decode → aggregate.rs → encode
      side stores behind SessionSideStore: GeminiShadowStore, CodexChatHistoryStore, reasoning_bridge
```

### 2.3 AppAdapter registry (target)

```
                 ┌───────────────────────── apps/registry.rs ─────────────────────────┐
                 │ APPS: [AppDescriptor{ id, slug, aliases, mode, caps, proxy_route,  │
                 │                       input_token_semantics, default_visible }]    │
                 └───────┬─────────────────────┬───────────────────────┬──────────────┘
      AppType::as_str/FromStr/all()     get_app_registry (IPC)   APPS.filter(caps.X) loops
                         │                     │                 (startup, tray, MCP import,
                         ▼                     ▼                  proxy startup, takeover)
             adapter(app) -> &'static dyn AppAdapter      src/entities/app/registry.ts
             ┌──────────────────────────────────────┐     (caps from backend + icon/colors/
             │ config_dir · prompt_file · skills_dir│      lazy form module in TS)
             │ read_live · write_provider (atomic)  │
             │ remove_provider · import_live        │
             │ credentials · common_snippet         │
             │ mcp()      -> Option<&McpProjector>  │──▶ read_servers / upsert / remove
             │ takeover() -> Option<&ProxyTakeover> │──▶ apply / has_placeholder / cleanup / preserve_in_backup
             │ session_source()                     │
             └──────────────────────────────────────┘
   impls: claude │ claude_desktop │ codex │ gemini │ grok │ opencode │ openclaw │ hermes
   persistence: AppSet / BTreeMap<AppType,_> (serde reads old flat keys); join tables mcp_server_apps, skill_apps
   enforced by: tests/app_adapter_conformance.rs (loops over APPS)
```

### 2.4 Frontend (`src`)

- `shared/bindings.ts` is generated. Raw `invoke` is banned outside it and `src/lib/api` by ESLint.
- `entities/app/registry.ts`: `useAppCaps()`, `appLabel()`. It replaces `VALID_APPS` (App.tsx:124) and the capability chains.
- `entities/<provider|mcp|skill|proxy|usage>/keys.ts`: query-key factories plus `invalidateProviderGraph()`.
- `features/providers/forms/<app>/{schema,toSettingsConfig,fromProvider,Fields}.tsx`. `ProviderForm.tsx` becomes a shell of 300 lines or fewer using `useFormContext`, with a shared `BasePreset` and presets loaded lazily.
- `app/` holds the shell: a typed view map, `React.lazy` views, lazily loaded locales, and typed i18n keys checked for locale parity.

---

## 3. Phased roadmap

Effort per workstream: S is under 1 week, M is 1 to 3 weeks, L is 3 to 6 weeks, XL is over 6 weeks (engineer-weeks).

### Phase 0 (v3.18): Safety net and real CI gates (3-4 weeks)

**Goal:** pin current behavior in the areas that regress most, and make CI enforce what CONTRIBUTING.md documents. Ship four zero-risk hotfixes.

| Workstream | Files/modules | Effort |
|---|---|---|
| **Golden bridge corpus** | <ul><li>Add `src-tauri/tests/proxy_golden.rs` and `tests/fixtures/proxy/{anthropic__openai_chat, anthropic__responses, anthropic__gemini, responses__chat, responses__anthropic}/<case>/` containing client_request, upstream_request.expected, upstream_response.sse/json, client_response.expected, usage.expected and meta.toml (intent plus source commit).</li><li>Add a `proxy::providers::testkit` module under `cfg(any(test, feature="testkit"))`.</li><li>Use insta snapshots with id/timestamp redaction.</li><li>Re-split each input at every byte offset (cases of 4 KB or less), at 200 random splits, and at UTF-8 and CRLF boundaries.</li><li>Check structural invariants: added/done pairing, stable tool ids, Chat index order, reasoning before message, exactly one terminal event, usage conservation.</li><li>Port the `concat!()` cases from streaming_codex_chat.rs, streaming_responses.rs, streaming_gemini.rs, streaming.rs and streaming_codex_anthropic.rs, plus regressions for 6d316c0, 27ce0a5, 650905a, 613fef7 and f991726.</li><li>Record the Gemini finishReason drift (streaming_gemini.rs:18 vs transform_gemini.rs:1142) as an expected-diff fixture.</li></ul> | M |
| **Cost golden table** | <ul><li>Rows: (app_type × {LEGACY, cache-inclusive, cache-exclusive} × usage × pricing row) → expected cost.</li><li>Run every row through `proxy/usage/calculator.rs`, `logger.rs`, `sql_helpers::fresh_input_sql` and the backfill (`usage_stats.rs:1783-1871`).</li><li>The grokbuild backfill row fails today; mark it `#[ignore]` with an issue link.</li></ul> | S |
| **Migration harness** | <ul><li>Build `tests/fixtures/db/v{0..15}.sql` from release tags (CI needs a full fetch; this clone has 50 commits).</li><li>Add `tests/migrations.rs` asserting user_version == 15, normalized `sqlite_master`/`table_info` equal to a fresh DB (reuse database/tests.rs:130-153), data preserved, and that migrating twice is a no-op.</li></ul> | M |
| **Config-writer snapshots** | <ul><li>Add `tests/config_writers.rs` and `tests/fixtures/live/**` for the 11 writers: claude settings.json, ~/.claude.json MCP, codex config.toml/auth.json, gemini .env/settings.json, grok, opencode, openclaw, hermes, claude_desktop, claude_plugin.</li><li>Inputs include comments, unknown keys, CRLF and BOM.</li><li>Generalize hermes_roundtrip.rs.</li></ul> | M |
| **Hermetic TestEnv and ProxyHarness** | <ul><li>`TestEnv { home: TempDir, db, settings }` with an injected `Paths` root in config.rs and settings.rs. This replaces the global HOME and test_mutex in tests/support.rs:7-67.</li><li>The production default `Paths` must equal today's `dirs::home_dir` behavior, enforced by a test.</li><li>`ProxyHarness::spawn(MockUpstream)` runs an axum mock on 127.0.0.1:0.</li><li>`tests/proxy_e2e.rs` covers two-provider failover, breaker open and half-open, each rectifier, SSE passthrough and `proxy_request_logs` rows.</li><li>Record baseline latency, allocations and DB queries per request.</li></ul> | L |
| **CI gates** (`.github/workflows/ci.yml`, `release.yml`) | <ul><li>`cargo clippy --all-targets -D warnings`.</li><li>`eslint.config.js` (typescript-eslint, react-hooks, @tanstack/query, no-restricted-imports for `invoke`) at warn level with a baseline, plus a `lint` script.</li><li>Prettier also covers `tests/`.</li><li>`cargo deny` with `src-tauri/deny.toml`.</li><li>cargo-llvm-cov and `vitest --coverage`, report-only.</li><li>An i18n parity script, plus filling zh-TW's 30 missing keys.</li><li>release.yml requires a green CI run on the tag SHA.</li><li>PR template and labeler rule: a change under `proxy/**` needs a fixture change or the `no-fixture-needed` label.</li></ul> | M |
| **Hotfixes** (no harness needed) | <ul><li>`RunEvent::Opened` (lib.rs:1611-1650) routes through `handle_deeplink_url`, which adds redaction and the Linux nudge.</li><li>`open_provider_terminal` (misc.rs:2719-2775) writes with 0600 permissions under app_config_dir/tmp, sanitizes the id and deletes the file after launch.</li><li>Add `session_log_sync` to `SYNC_SKIP_TABLES`/`SYNC_PRESERVE_TABLES` (backup.rs:15-30).</li><li>Run session sync under `spawn_blocking` (lib.rs:1093).</li></ul> | S |

**Exit criteria**
- At least 60 golden fixtures across all 5 bridges, stream and non-stream, all passing under the chunk-split rules above. The roughly 1046 existing inline proxy tests stay green.
- The cost golden table covers 8 apps × 3 semantics. The only failures are the known grokbuild backfill rows, marked `#[ignore]`.
- The migration harness passes from at least 10 distinct start versions.
- Config-writer snapshots exist for all 11 writers, and untouched sections are byte-identical after a round-trip.
- `test_mutex` is gone, integration tests run in parallel, and proxy_e2e has at least 8 scenarios, green on 3 OSes.
- CI enforces clippy --all-targets, ESLint (no new warnings over the baseline), cargo-deny and i18n parity. A tag without a green CI run cannot publish.
- No info-level log line contains an unredacted `ccswitch://` URL.

### Phase 1 (v3.19): "Trust release", security and correctness fixes with no structural change (2-3 weeks)

**Goal:** close the concrete security holes and data-integrity bugs before any refactor makes diffs larger. Each fix comes with a regression test from a Phase 0 harness.

| Workstream | Files/modules | Effort |
|---|---|---|
| **IPC input hardening** | <ul><li>`launch_session_terminal` (commands/session_manager.rs:28) takes `{provider_id, session_id}` and builds the resume command on the backend from session metadata.</li><li>These commands validate path arguments against allowed roots or accept a dialog-issued `PickedPath(Uuid)` token, following the pattern at session_manager/mod.rs:131: `restore_env_backup` (env.rs:20), `export/import_config_*` (import_export.rs:21-60), `install_skills_from_zip` (skill.rs:326) and `set_app_config_dir_override` (settings.rs:296).</li></ul> | M |
| **Tauri capabilities** | <ul><li>build.rs uses `AppManifest::commands`, with permissions in `src-tauri/permissions/<domain>.toml`.</li><li>`capabilities/default.json` lists commands explicitly, and privileged commands go in a separate set.</li><li>Tighten CSP `connect-src`/`img-src` in tauri.conf.json:29 where feasible.</li></ul> | M |
| **Atomic writes** | <ul><li>Route settings.rs:680-697, claude_plugin.rs:83/117, services/env_manager.rs:139/220 and misc.rs:2773 through `config::atomic_write` (config.rs:297), adding a variant that preserves file mode.</li><li>Add `src-tauri/clippy.toml` with `disallowed-methods = ["std::fs::write"]`; test modules are exempt.</li></ul> | S |
| **Accounting correctness** | <ul><li>`AppType::input_semantics()` becomes the single source, used in calculator.rs:62, logger.rs:75, sql_helpers.rs:19, usage_stats.rs:377 and :1820. This fixes the grokbuild backfill.</li><li>Session importers persist `input_token_semantics` and `pricing_model`.</li><li>Fix or delete the dead Gemini UPSERT (session_usage_gemini.rs:263/300-318).</li><li>Remove the per-read reseed at commands/usage.rs:127, and add a `deleted` tombstone column (the first numbered migration, v16).</li></ul> | M |
| **Spend limits** | <ul><li>`check_provider_limits` (usage_stats.rs:1599-1680) uses `[start,end)` ranges computed in Rust (reusing `compute_local_midnight_cutoff`).</li><li>Add an index on `proxy_request_logs(provider_id, app_type, created_at)`.</li><li>Propagate errors instead of `unwrap_or(0.0)`.</li></ul> | S |
| **DB robustness** | <ul><li>Set WAL, `synchronous=NORMAL` and `busy_timeout=5000` in `Database::init`, after verifying `snapshot_to_memory`, WebDAV/S3 export and `update_hook` under WAL. Keep DELETE journal mode on non-local paths.</li><li>Move the pre-migration backup ahead of `create_tables` (mod.rs:123-137).</li></ul> | S |

**Exit criteria**
- No `#[tauri::command]` accepts a free-form shell string. Every listed path-taking command has a test that rejects a path outside the allowed roots.
- Privileged commands are not in the default capability set.
- The cost golden table is fully green, including the grokbuild backfill. A deleted built-in price stays deleted after restart and list (integration test).
- `disallowed-methods` passes with zero non-test `std::fs::write`, and the Phase 0 writer snapshots are unchanged.
- Startup reports `PRAGMA journal_mode = wal`. Spend-limit queries show an index seek in EXPLAIN QUERY PLAN, and a DB error surfaces as an error rather than "$0".
- A test shows sync import preserves the local `session_log_sync` rows.

### Phase 2 (v3.20): Typed vocabulary, registries, IPC contract and bug-report capture (3-4 weeks)

**Goal:** add the shared types every later move depends on, next to the existing code, so the compiler checks dispatch. Ship the capture tool that turns user reports into fixtures.

| Workstream | Files/modules | Effort |
|---|---|---|
| **Proxy vocabulary** | <ul><li>`proxy/protocol/format.rs`: `WireFormat`, `ClaudeApiFormat` (replacing the `&str` values from claude.rs:38-96), `AppRef`, and `BreakerKey` (replacing `split(':')` at provider_router.rs:250).</li><li>provider_router uses `From<&AppProxyConfig>` (circuit_breaker.rs:52).</li><li>Delete `ApiFormat` (types.rs:121), response_handler.rs, the HandlerConfig constants, `ProviderAdapter::transform_response`, `ProviderType::needs_transform` and `get_adapter_for_provider_type`.</li><li>Session-id extraction lives only in session.rs.</li></ul> | M |
| **App registry** | <ul><li>`apps/registry.rs` with `AppDescriptor`/`APPS`. `AppType` as_str, FromStr, is_additive_mode and all() delegate to it.</li><li>Replace the hard-coded lists at lib.rs:1754, services/proxy.rs:1308, tray.rs:157 and lib.rs:788-819.</li><li>Add the `get_app_registry` command.</li><li>Fix the `unreachable!` (prompt_files.rs:31/38) and the bogus Claude Desktop skills dir (skill.rs:544).</li><li>Exhaustiveness test: every variant has a descriptor, i18n label, icon and presets file.</li></ul> | M |
| **Typed errors** | <ul><li>`IpcError{code: ErrorCode, message, details, i18n_key}` with `From<AppError>` (keeping `Localized.key`) and a temporary `From<String>` mapping to `UNKNOWN`.</li><li>Replace `format_skill_error` (error.rs:125).</li><li>errorUtils.ts:53-64 and skillErrorParser.ts switch on `code`.</li></ul> | M |
| **Bindings** | <ul><li>Time-boxed tauri-specta spike in week 1 on the usage domain. Fallback: ts-rs plus a generated command-name map.</li><li>Derive `Type` on Provider, AppType, ProxyStatus, IpcError and the usage DTOs.</li><li>Generate `src/shared/bindings.ts` with a CI dirty check, and type `tests/msw/handlers.ts` against it.</li><li>Normalize casing: `queryProviderUsage`, `testUsageScript` and the 41 non_snake_case args.</li><li>Wire up or delete the 14 orphan commands.</li></ul> | L |
| **Frontend registry** | <ul><li>`src/entities/app/registry.ts` with `useAppCaps`/`appLabel`.</li><li>Fix the "OpenCode" fallback labels (useProxyStatus.ts:128, ProxyToggle.tsx:35).</li><li>Replace `VALID_APPS` and the capability chains in App.tsx:124 and 224-302.</li><li>ESLint ratchet bans new app-id literal comparisons (283 today).</li></ul> | M |
| **Feature: bug-report capture** | <ul><li>Opt-in, time-boxed `CaptureTap` that writes the redacted client request, upstream request, upstream SSE (with timings) and client SSE in the golden fixture layout.</li><li>An "Export bug report" button in the request log.</li><li>A redaction test over a corpus of key patterns.</li></ul> | M |

**Exit criteria**
- `"openai_responses"`/`"openai_chat"`/`"gemini_native"` appear only in format.rs parse/serialize, at 10 occurrences or fewer. `#[allow(dead_code)]` in `proxy/` drops from 72 to 15 or fewer.
- No hard-coded app list remains at lib.rs:1754, proxy.rs:1308, tray.rs:157 or App.tsx:124, and the exhaustiveness test passes.
- At least 50% of commands return `IpcError`, and errorUtils.ts has no Chinese-substring matching.
- bindings.ts is generated and checked in CI, or the ts-rs fallback decision is written down. Raw `invoke()` outside the bindings/api layer is an ESLint error.
- Frontend app-literal comparisons are at 200 or fewer.
- Exported captures contain no keys.

### Phase 3 (v3.21): Proxy request path (endpoint table, RoutePlan, attempt pipeline, canonical usage) (5-7 weeks)

**Goal:** make one decision per attempt that drives both request encoding and response decoding. Break up the god functions `forward_with_retry_inner` and `forward`. Write the cache-token arithmetic and the stop-reason mapping once each. All golden fixtures must stay identical.

| Workstream | Files/modules | Effort |
|---|---|---|
| **Endpoints and handler** | <ul><li>`proxy/endpoints.rs` holds the `ClientEndpoint` table, and server.rs builds its routes from it.</li><li>One `handle_proxied` replaces handlers.rs:156/692/779/894/1723, and the compact Responses copy (894-985) is deleted.</li><li>`proxy/errors/render.rs` has one renderer per client format.</li></ul> | M |
| **Cached runtime config** | <ul><li>`ProxyState.runtime_config: ArcSwap<RuntimeConfigSnapshot>`, refreshed in `apply_runtime_config`/`update_config`.</li><li>handler_context.rs:95-110 and provider_router.rs:43/134 stop reading the DB.</li><li>`RequestForwarder { shared: Arc<ForwarderDeps>, req: RequestScope }` replaces the 17-argument constructor.</li></ul> | M |
| **RoutePlan** | <ul><li>`proxy/routing/plan.rs` absorbs `resolve_claude_api_format` (forwarder.rs:2432), `should_convert_codex_responses_to_*`, the endpoint rewrites, gemini_url and the copilot/codex/bedrock booleans (forwarder.rs:1129-1412).</li><li>`ForwardResult` carries the plan, so handlers.rs:229, 837-858 and 954-975 stop re-deriving it.</li><li>The plan is logged for each attempt.</li></ul> | M |
| **Attempt pipeline** | <ul><li>`pipeline/failover.rs`: `AttemptRecorder` replaces the 4 `try_switch` copies and the 9 `success_rate` copies.</li><li>`pipeline/recovery.rs`: `RecoveryStrategy` with MediaDowngrade, ThinkingSignatureRectifier and ThinkingBudgetRectifier, replacing the nested `forward()` calls at 579/725/891.</li><li>`request_builder.rs` (BodyStages), `headers.rs` (a declarative HeaderPolicy instead of the `saw_*` flags), `transport.rs` and `validate.rs`.</li><li>forwarder.rs becomes a facade.</li></ul> | L |
| **Canonical usage and stop reasons** | <ul><li>`proxy/usage/ir.rs`: `UsageIR{fresh_input, cache_read, cache_write, output, reasoning, semantics}` with `from_<format>`/`to_<format>`.</li><li>It replaces transform_responses.rs:431, transform_codex_chat.rs:1672, transform_codex_anthropic.rs:144 and transform_gemini.rs:1101, and `UsageParser::from_*` delegates to it.</li><li>A canonical `StopReason` table per format. The Gemini unification ships as its own PR that updates the fixtures.</li><li>proptest checks conservation of billable tokens across format pairs.</li></ul> | M |
| **Stream framing** | <ul><li>`proxy/stream/sse_decoder.rs` is one decoder replacing the 3 sniffers (streaming_responses.rs:336, forwarder.rs:2882, handlers.rs:1910).</li><li>Wrap the existing converters behind `StreamDecoder`/`StreamEncoder` traits; do not rewrite them yet.</li><li>`UsageTap` keeps running totals, replacing the re-parse and `Vec` in response_processor.rs:677-790.</li><li>`TimingTap`.</li><li>`aggregate.rs` replaces handlers.rs:1814/2022.</li></ul> | L |
| **Features** | <ul><li>Per-request **trace inspector**: a ring buffer of RoutePlan, attempts, recovery strategies applied, TTFB and usage source.</li><li>**Usage parity badge**: shown when the normalized `UsageIR` disagrees with the raw upstream usage block.</li></ul> | M |

**Exit criteria**
- Every golden fixture, including upstream_request.expected, is byte-identical after normalization, or the diff is a documented intended fix. proxy_e2e is green on 3 OSes.
- forwarder.rs is 600 lines or fewer, with no function in `pipeline/` over 150 lines. handlers.rs is 1200 lines or fewer. `forward()` has exactly one call site.
- A new E2E test fails over from a Copilot provider (openai_responses) to an Anthropic provider and decodes the response with the plan's format.
- Zero DB reads on the hot path, measured by a counter in proxy_e2e. p50 overhead is no worse than the Phase 0 baseline, and UsageTap memory no longer grows with the event count.
- Cache-token arithmetic appears only in `UsageIR::from_*`. Each bridge's stream and non-stream paths return the same stop_reason.

### Phase 4 (v3.22): Per-app adapters, takeover engine and persistence (6-8 weeks)

**Goal:** collapse the per-app fan-out and give usage, pricing and schema a single owner each, one app or one table per PR. Then use the new seams to ship takeover for additive apps and OpenClaw MCP.

| Workstream | Files/modules | Effort |
|---|---|---|
| **AppAdapter** | <ul><li>`apps/adapter.rs`. Migrate Gemini first, then Grok, Claude, Codex, OpenCode, OpenClaw, Hermes and ClaudeDesktop, one PR each.</li><li>`live.rs` write_live_snapshot (:1016-1170), read_live_settings (:1309), import_default_config (:1427) and common-config (:485-665) become adapter calls, as do prompt_files.rs, skill.rs:500-550, commands/config.rs:90-180 and provider.rs:147-200.</li><li>`settings::override_dir(app)` replaces the 7 `get_<app>_override_dir` functions.</li></ul> | XL |
| **McpProjector** | <ul><li>`mcp/common.rs` holds one `collect_enabled_servers` (now duplicated in claude.rs:18, codex.rs:23 and gemini.rs:18).</li><li>One `import_from_app` replaces services/mcp.rs:294-460 and drops the `MultiAppConfig` dependency.</li><li>lib.rs:788-815 becomes a loop.</li></ul> | M |
| **Takeover engine** | <ul><li>Split services/proxy.rs into `services/proxy/{mod,lifecycle,takeover/{claude,codex,gemini,grok},backup,hot_switch,config}.rs`.</li><li>`takeover(app, Strict\|BestEffort)` replaces :1528-1712. Backup, restore and cleanup become loops.</li><li>The base URL comes from `descriptor.proxy_route` (fixing the 5 copies).</li><li>`TakeoverError` enum, and `ProxyTakeoverStatus` generated from the registry.</li><li>Gate: crash/restore E2E for each proxy-capable app before code moves, plus a runtime flag that falls back to the old path for one release.</li></ul> | L |
| **Per-app state** | <ul><li>`AppSet` replaces McpApps/SkillApps. `CommonConfigSnippets`, `VisibleApps`, `current_providers` and `config_dirs` become maps whose serde reads the old flat keys.</li><li>Migration adds `mcp_server_apps` and `skill_apps` join tables and drops the `proxy_config` CHECK.</li><li>dao/mcp.rs and dao/skills.rs stop naming apps and stop using positional `row.get`.</li><li>Old columns are written alongside the new tables for one release, so mixed-version WebDAV/S3 sync keeps working.</li></ul> | L |
| **Migration framework** | <ul><li>`database/migrations/` holds the ordered `MIGRATIONS` table with a frozen baseline and a `schema_migrations` audit table.</li><li>The 8 swallowed ALTERs and the repairs in `create_tables` become numbered steps that propagate errors, and `create_tables` is deleted.</li><li>CI check: the table is contiguous and ends at `SCHEMA_VERSION`.</li></ul> | M |
| **Pricing and usage repos** | <ul><li>`pricing/{catalog.json,resolver.rs,engine.rs}`. The resolver moves out of usage_stats.rs:2004-2275. The hand-written backfill is deleted, and the roughly 880-line seed and repair list in schema.rs:1516-2745 is removed.</li><li>Provenance columns: source, catalog_version, updated_at, deleted.</li><li>`database/repo/{usage,pricing,session_cursor}.rs` with a `UsageQuery` builder and keyset pagination.</li><li>`UsageSource` enum. Ingest writes one transaction per file together with its cursor.</li><li>A `superseded_by` column, filled once by a backfill that runs the current EXISTS logic, replaces the correlated EXISTS (usage_stats.rs:297-336).</li><li>Rollup cost stored as integer micro-USD. `RetentionPolicy` plus `Maintenance::run` replace the literals at mod.rs:148 and backup.rs:271.</li></ul> | XL |
| **Features** | <ul><li>Proxy takeover, failover and usage for **OpenCode, OpenClaw and Hermes** (rewriting only the selected entry's baseUrl).</li><li>**OpenClaw MCP** (Issue #4834).</li><li>**Capability matrix** page.</li><li>**Live-config drift detection** with a "reconcile" action.</li><li>**Budget alerts** on `usage-log-recorded`.</li><li>**Retention and DB-size** panel.</li></ul> | L |

**Exit criteria**
- live.rs, prompt_files.rs, skill.rs and commands/config.rs contain no `match app_type`. Backend `match app` sites outside `apps/**` number 15 or fewer (from about 74-79).
- The conformance suite passes for all 8 apps. It checks credential round-trips, MCP upsert and remove, prompt and skills paths, takeover apply followed by cleanup restoring the original bytes, and that caps match the implemented traits.
- `services/proxy/` has no file over 1500 lines of production code. Crash/restore E2E is green for claude, codex, gemini, grokbuild, opencode, openclaw and hermes.
- The fresh DB and every migrated vN DB are schema-equivalent. A test-only dummy 9th app touches 6 or fewer backend files and needs no migration.
- One cost function exists. The cost golden table (at least 50 rows) passes, and backfill output equals ingest output for the same row.
- No SQL outside `database/` (grep in CI). Dashboard queries have no correlated subquery. The dashboard summary p95 is 50 ms or less on a 1M-row synthetic DB.
- A test that kills the process between files during session import shows no double counting.

### Phase 5 (v4.0): Canonical protocol IR and codecs, plus command-layer cleanup (8-10 weeks, one bridge per release candidate)

**Goal:** fix the root cause of bridge drift by expressing every bridge as `decode(upstream) → IR → encode(client)`. Finish the command and shell extraction.

| Workstream | Files/modules | Effort |
|---|---|---|
| **IR and codecs** | <ul><li>`proxy/protocol/ir.rs` (ChatRequest, Part, StreamEvent) reusing `UsageIR`/`StopReason`.</li><li>`proxy/codec/{anthropic,openai_chat,openai_responses,gemini}/{request,response,stream}.rs`. Non-stream responses are stream plus aggregate.</li><li>Migration order: anthropic←openai_chat, then gemini, then responses↔anthropic, then responses→chat.</li><li>Each bridge deletes its old transform*/streaming* files only after golden parity passes and a release ships without regressions.</li><li>`proxy.codecEngine=legacy` flag kept for one release.</li></ul> | XL |
| **SessionSideStore** | GeminiShadowStore, CodexChatHistoryStore and reasoning_bridge move behind one trait that the codecs receive. | M |
| **Command layer** | <ul><li>Split misc.rs into `services/cli_tools/{search_paths,version_probe,installations,lifecycle_cmd,remote_versions}.rs` and `platform/terminal/{macos,linux,windows}.rs`, merged with session_manager/terminal.</li><li>Domain folders with `collect_commands!`.</li><li>Move `reset_circuit_breaker`'s policy (proxy.rs:349-406) into a FailoverService and `query_provider_usage_inner` (provider.rs:421-676) into `services/usage_query.rs`.</li><li>A Ctx/EventSink seam, deleting the `*_internal`/`*_test_hook` triplets.</li><li>CI fails on a command body over 40 lines or direct `state.db` access.</li></ul> | L |
| **App shell** | <ul><li>`app_shell/bootstrap.rs` StartupStep pipeline with a `get_startup_report` command.</li><li>`run_events.rs`.</li><li>`tray/{model,menu,events}.rs` driven by an event bus.</li><li>The lightweight `AtomicBool` moves into AppState.</li></ul> | M |
| **Features** | <ul><li>**Cross-platform session resume** (Windows/Linux).</li><li>**Pre-content mid-stream failover**: buffer until the first TextDelta/ToolCall, then retry on an early error or `response.failed`.</li><li>**Usage parity checker** run in CI over the corpus.</li><li>A first new client→upstream combination (Gemini CLI → Anthropic upstream).</li></ul> | L |

**Exit criteria**
- All 5 bridges run on codecs, the old `providers/transform*.rs`/`streaming*.rs` files are deleted, and the net reduction is at least 8k lines. Fixtures pass, or any diff is documented and reviewed.
- `cached_tokens`/`cache_creation_input_tokens` appear only in codec and usage modules. The parity checker reports 0 mismatches.
- commands/misc.rs is gone, no command file exceeds 600 lines, lib.rs `run()` is 300 lines or fewer, and no `*_test_hook` remains. 100% of commands return `IpcError`.
- `proxy/` and `database/` line coverage is at least 75%, ratcheted.
- Proxy fix commits per release are below the Phase 0 baseline window.

### Track F (parallel from Phase 2 bindings, lands by v4.0): Frontend decomposition (8-10 weeks)

| Workstream | Files/modules | Effort |
|---|---|---|
| **Golden tests first** | Per-app, per-preset `settingsConfig` golden snapshots written before any form logic moves. | S |
| **ProviderForm split** | <ul><li>`features/providers/forms/<app>/{schema,toSettingsConfig,fromProvider,Fields}.tsx`, with a base schema plus a discriminated union, and Fields using `useFormContext`.</li><li>A shared `BasePreset`.</li><li>A zod error map backed by i18n replaces the Chinese literals in schemas/provider.ts.</li><li>Feature: a **live diff preview** of the target live file before save.</li></ul> | XL |
| **Query keys** | <ul><li>`entities/<x>/keys.ts` and `invalidateProviderGraph()` replace the 23 inline `["providers", appId]` keys and the triplicated blocks in failover.ts and mutations.ts.</li><li>Per-query staleTime instead of the global staleTime 0 plus refetchOnWindowFocus.</li></ul> | M |
| **Shell and perf** | <ul><li>App.tsx split into AppHeader, a ViewTitle map and confirm orchestration.</li><li>`React.lazy` for SessionManagerPage, settings sync, UsageScriptModal, the skills/MCP/proxy panels and the CodeMirror editors.</li><li>Lazy per-app presets and locales.</li><li>Typed i18n (CustomTypeOptions), removing the 691 inline `defaultValue` fallbacks.</li></ul> | L |
| **UI primitives and tests** | <ul><li>Custom `fixed inset-0` overlays move to Radix Dialog.</li><li>Component tests for CodexFormFields, HermesFormFields, OpenClawFormFields, ProxyPanel and UsageScriptModal.</li></ul> | M |
| **E2E** | tauri-driver plus WebdriverIO Linux job (xvfb): first run, add a preset, switch and check `~/.claude/settings.json`, then takeover on and restore. Runs on the AppImage before upload. | M |

**Exit criteria**
- ProviderForm.tsx and App.tsx are each 300 lines or fewer.
- App-literal comparisons in `src/` are 40 or fewer, as an ESLint error.
- The settingsConfig goldens are unchanged.
- The initial JS bundle is at least 30% smaller than the v3.17 baseline.
- The tauri-driver smoke is green and blocks a release when it fails.

---

## 4. Refactor catalog

| Title | Area | Effort | Risk | Depends on | Payoff |
|---|---|---|---|---|---|
| Golden bridge corpus and chunk-split testkit | Proxy/tests | M | Low | none | Pins the main source of regressions; one fixture per future bug |
| Cost golden table | Usage/tests | S | Low | none | Exposes the 4-way cost drift (grokbuild backfill) |
| Migration snapshot and equivalence harness | DB/tests | M | Low | full git fetch | Every upgrade path tested on each PR |
| Config-writer round-trip snapshots | Apps/tests | M | Low | insta | Guards users' live CLI configs |
| Hermetic TestEnv and ProxyHarness E2E | Tests | L | Medium | none | Parallel tests; first true end-to-end proxy coverage |
| Atomic writes and the fs::write lint | Config | S | Low | snapshots | Makes the README's atomic-write claim true |
| `AppType::input_semantics()` single source | Usage | S | Low | cost table | Fixes the grokbuild backfill; ends drift |
| Capability-scoped commands and PickedPath | IPC/security | M | Medium | none | A renderer XSS no longer leads to code execution or arbitrary file I/O |
| WAL, busy_timeout and early pre-migration backup | DB | S | Medium | WAL verification | Less lock contention; a real pre-migration backup |
| WireFormat/AppRef/BreakerKey and dead-code purge | Proxy | M | Low | Phase 0 | Compiler-checked dispatch; about 57 dead_code attributes removed |
| AppDescriptor registry | Apps | M | Low | none | About 15 hand-kept app lists removed; base for everything per-app |
| IpcError with stable codes | IPC | M | Medium | none | Error handling survives message rewording |
| Generated bindings (tauri-specta, ts-rs fallback) | IPC/FE | L | Medium | domain registration | About 2.9k lines of hand TS removed; drift becomes a compile error |
| Endpoint table and single handle_proxied | Proxy | M | Low | vocabulary | About 600 duplicated lines removed |
| ArcSwap RuntimeConfigSnapshot | Proxy | M | Low | endpoints | 4-6 DB reads per request removed |
| RoutePlan resolved once per attempt | Proxy | M | Medium | vocabulary | Request and response format can no longer disagree after failover |
| AttemptRecorder and RecoveryStrategy | Proxy | L | Medium | RoutePlan, E2E | Retry and accounting fixes land once |
| UsageIR and canonical StopReason | Proxy/usage | M | Medium | fixtures with usage | Single cache-token arithmetic; ends stream vs non-stream drift |
| SseDecoder, StreamDecoder/Encoder and taps | Proxy | L | Medium | RoutePlan, chunk fuzzing | One framing implementation; no double parse on the usage hot path |
| AppAdapter trait and per-app modules | Apps | XL | Medium | registry, snapshots | About 40 matches collapse; a new app is one module |
| McpProjector and generic MCP loops | Apps/MCP | M | Low | registry | About 300 copy-pasted lines removed; OpenClaw MCP becomes one impl |
| ProxyTakeover engine and services/proxy split | Apps/proxy | L | High | adapter, crash E2E | One takeover implementation instead of 3 variants × 4 apps |
| Keyed per-app state and join tables | Apps/DB | L | Medium | migration framework | No schema migration needed per new app |
| Versioned migration framework | DB | M | Medium | harness | About 1.3k lines out of schema.rs; no swallowed ALTERs |
| Pricing catalog, provenance and engine | Pricing | L | Medium | migrations | One cost function; deletions persist; price updates ship as data |
| UsageRepo, ingest batching and superseded_by | DB/usage | XL | Medium | repo, backfill | Correlated EXISTS removed; crash-consistent ingest |
| Canonical IR and 4 codecs | Proxy | XL | High | UsageIR, stream layer, corpus | At least 8k lines removed; each rule written once |
| misc.rs split and unified terminal stack | Commands | L | Medium | none | Process spawning in one audited module |
| Thin commands with Ctx/EventSink | Commands | L | Medium | domain split | Services testable without Tauri; triplets deleted |
| App shell StartupStep and deep-link router | Shell | M | Medium | none | Testable startup; a single redaction point |
| Tray model/menu/events split | Shell | M | Low | EventSink | Tray state can no longer drift from UI state |
| Frontend registry and literal ratchet | FE | M | Medium | get_app_registry | 283 literal comparisons down to 40 or fewer |
| ProviderForm per-app modules | FE | XL | High | settingsConfig goldens | 2578 lines become a 300-line shell |
| Query-key factories | FE | M | Low | none | Fewer stale-cache bugs |
| Lazy views and locales; typed i18n | FE | M | Low | none | 30% or more off the initial bundle; safe key renames |

---

## 5. Feature backlog

| Title | Area | Effort | Why |
|---|---|---|---|
| Capture and export bug report (Phase 2) | Proxy | M | User reports become golden fixtures; bridge bugs today cannot be replayed |
| Usage parity badge and CI checker (Phases 3/5) | Usage | S | Flags double-counted cache_read and zero-usage streams before users see wrong bills |
| Per-request trace inspector (Phase 3) | Proxy/UI | M | Shows why a request went to provider B; today this is only in logs |
| Takeover for OpenCode/OpenClaw/Hermes (Phase 4) | Apps/proxy | M | Hard-coded to false today (services/proxy.rs:714-716), although wire routing already exists |
| OpenClaw MCP (Phase 4) | MCP | S | Issue #4834; a stub today (services/mcp.rs:142) |
| Capability matrix page (Phase 4) | Settings | S | Users find missing capabilities by trial and error; doubles as a support diagnostics view |
| Live-config drift detection and reconcile (Phase 4) | Apps | M | Repairs leftover takeover placeholders after a crash, for all 8 apps |
| Budget alerts (Phase 4) | Usage | S | Limits are pull-only today and used to fail open |
| Retention and DB-size panel (Phase 4) | Settings | S | Detail retention is hard-coded to 30 days; rollups are never pruned |
| Persisted quota snapshots and burn-rate forecast | Usage | M | UsageCache is in-memory only (usage_cache.rs:3) |
| Time-versioned pricing and audited recompute | Pricing | M | Price corrections never reach already-priced history |
| Cross-platform session resume (Phase 5) | Sessions | M | macOS-only today, though Linux and Windows launchers exist in misc.rs |
| Pre-content mid-stream failover (Phase 5) | Proxy | M | Catches gateways that return 200 and then an immediate error |
| Startup diagnostics report (Phase 5) | Shell | S | A failed migration or live import is currently a silent "missing providers" |
| Actionable error toasts (after IpcError) | FE | M | TOOL_NOT_INSTALLED offers Install; CIRCUIT_OPEN offers Reset |
| Provider config diff preview before save (Track F) | FE | M | Fewer misconfigured live files |
| Command palette (cmdk is already a dependency) | FE | S | Replaces scattered shortcuts across 8 apps |
| Bridge doctor / provider conformance probe | Proxy | M | Sets VendorQuirks before failures happen, instead of reactive rectifiers |
| Deep-link import preview with trust indicator | Security | M | Reduces phishing risk from malicious ccswitch:// links |
| `cargo xtask new-app` scaffolder | Apps | S | Only after one real app has been added through AppAdapter |
| New client→upstream combinations | Proxy | M | The IR makes Gemini/OpenCode/Hermes → Anthropic/Responses upstreams cheap |

---

## 6. Testing and quality gates plan

**Test layers**
1. **Golden bridge fixtures** (`tests/fixtures/proxy/<bridge>/<case>`).
   - Snapshots plus structural invariants, under every-offset, random, UTF-8 and CRLF chunk splitting.
   - Each case has a `meta.toml` with intent and source commit.
   - Invariants are asserted separately from snapshots, so reviewers cannot approve a broken shape by accepting a snapshot diff.
2. **Cost golden table**: app × semantics × usage × price, run through every cost path. After Phase 4 there is only `pricing::engine`.
3. **Migration harness**: `v{0..15}.sql` fixtures plus every new migration, checking schema equivalence, data preservation, idempotence, and that a vN DB is rejected by vN-1.
4. **Config-writer snapshots**: 11 writers; untouched sections byte-stable; inputs with comments, unknown keys, CRLF and BOM.
5. **Proxy E2E** (`ProxyHarness` plus axum mock).
   - Covers failover, breaker, each RecoveryStrategy, mid-stream failover, takeover crash/restore and usage rows.
   - Counts DB queries per request and checks latency and allocation baselines.
6. **Adapter conformance** (`tests/app_adapter_conformance.rs`): loops over APPS, so a new app gets the same contract automatically.
7. **Property tests** (proptest): token conservation across format pairs; SSE decoder output is the same under any chunking.
8. **Frontend**: settingsConfig goldens per app and preset; component tests for the untested large forms; MSW typed against the bindings, with a test that each command has a handler or is allowlisted.
9. **tauri-driver smoke** on Linux debug builds and on the release AppImage.

**CI gates, ratcheted**

| Gate | Phase | Mode |
|---|---|---|
| `cargo clippy --all-targets -D warnings`, `disallowed-methods` | 0/1 | error |
| ESLint (react-hooks, query, no raw `invoke`, app-literal ratchet) | 0 → 2 | warn plus baseline → error |
| Prettier over `src/` and `tests/` | 0 | error |
| cargo-deny (advisories, licenses, bans) | 0 | error |
| i18n key parity | 0 | error |
| Coverage (llvm-cov plus vitest) | 0 → 5 | report → ratchet (75% proxy/database) |
| Bindings dirty check | 2 | error |
| MIGRATIONS contiguous and == SCHEMA_VERSION | 4 | error |
| No SQL outside `database/`; command body 40 lines or fewer | 4/5 | error |
| `proxy/**` change needs a fixture or the `no-fixture-needed` label | 0 | error |
| release.yml requires green CI on the tag SHA, plus tauri-driver smoke on the artifact | 0/F | blocking |

**Process rules**
- Every bug fix during the migration adds a fixture first.
- Every structural PR keeps the old path callable until parity is shown.
- Write a `docs/dev/ACCEPTANCE.md` playbook for "add an app" and "fix a proxy bug".
- KPI tracked in release notes: proxy fix commits per release.

---

## 7. Risks and mitigations

| Risk | Mitigation |
|---|---|
| Fixtures encode current bugs or miss real upstream quirks | Real redacted captures from the Phase 2 tool; meta.toml intent; known drifts (Gemini finishReason) kept as explicit expected-diff cases; intended fixes land as separate PRs |
| Codec IR migration loses information (opaque reasoning, thought signatures, Codex history restore) | One bridge per release candidate; SessionSideStore; `codecEngine=legacy` flag for one release; delete old code only after parity plus one release without regressions |
| Takeover consolidation corrupts users' settings.json, config.toml or auth.json | Atomic writes and snapshots land first; crash/restore E2E for each app before moving code; one app per PR; flag to fall back to the old path; pre-takeover backups kept |
| Irreversible DB changes (WAL, join tables, superseded_by, provenance) break upgrades or mixed-version WebDAV/S3 sync | Backup taken before any DDL; fixtures from every released version; old columns written alongside new tables for one release; backfill dry run plus a dashboard parity test; WAL disabled on non-local paths; beta channel first |
| Capability scoping and path tokens break legitimate flows | Inventory callers through the bindings; E2E for each privileged flow; allow-list the privileged set for the main window first, then narrow; release-note callout |
| tauri-specta does not fit Tauri 2 or the serde shapes | Week-1 spike on one domain; ts-rs fallback decided in advance; `settingsConfig` stays `Value` at the boundary |
| Merge contention with the high fix rate in proxy/ and services/proxy.rs | Small PRs to main behind facades; a freeze of a few days per large file move; phase ownership |
| Extra abstraction layers regress hot-path performance | Phase 0 latency, allocation and query baselines; bench comparison in CI; synchronous state-machine codecs with no per-event JSON re-parse |
| Scope overrun for a small team (30-38 engineer-weeks) | Each phase is a standalone release; Track F can slip without blocking; minimum cut = Phases 0-2 plus the Phase 4 registry and takeover |
| Hermetic `Paths` injection changes production path resolution | Default `Paths` equals today's `dirs::home_dir`; a test asserts identical resolved paths for every app |
| Shallow history (50 commits) limits DB fixtures and regression mining | Full fetch in CI; build vN.sql from release tags |

---

## 8. Explicit non-goals

- No big-bang rewrite and no long-lived refactor branch. Every phase merges incrementally and ships as a normal release.
- No change of stack: Tauri 2, React 18, react-query and rusqlite stay. No ORM, no async DB driver, no replacement for SQLite.
- No new bridge combinations before Phase 5 codec parity, except the one combination listed as a Phase 5 exit item.
- No 9th AI CLI until the Phase 4 conformance suite exists. The scaffolder is deferred until after one real addition through AppAdapter.
- No declarative or community manifest apps, no remote pricing or preset catalogs, no cross-app provider sharing in this horizon. The data shapes leave room for them later.
- No typed per-app Rust `Settings` across DB and IPC. `settings_config` stays `serde_json::Value` at the boundary, typed only inside adapters and frontend zod schemas.
- No latency-weighted routing or hedged requests. TimingTap only collects the data.
- No feature-sliced re-layout of `src/` as a standalone project. Files move when they are touched for registry or ProviderForm work.
- No change to the on-disk formats of users' CLI configs. The refactor must be invisible in `~/.claude`, `~/.codex`, `~/.gemini` and so on (byte-stable snapshots).
- No full Windows/macOS tauri-driver E2E in this plan; Linux only.
- No translation of the existing Chinese log and comment conventions. i18n work is limited to typed keys, parity, and removing `defaultValue` fallbacks.
---

## Appendix: Plan critique (corrections applied to the ticket set)

Plan checked against the code (read-only). It holds up mostly, but it misses several security areas and a few compatibility mechanisms, and some steps are scheduled before the things they depend on.

**(a) Areas the plan ignores**
- **Deep links can install scripts that run.** A `ccswitch://` link can carry a base64 `usage_script` (`src/deeplink/provider.rs:231-250`). The app runs it in rquickjs, which can make network calls with the provider's key (`src/usage_script.rs:1,51`). This is worse than the log-redaction hotfix, yet the only related item is the unphased "import preview" in the backlog. It belongs in Phase 1.
- **The local proxy has no real inbound auth.** The client token is the constant `PROXY_MANAGED` (`proxy/forwarder.rs:40`), and `listen_address` can be changed (`proxy/server.rs:101`). Any local process, any page using DNS rebinding, or any LAN host if it's bound to 0.0.0.0, can spend the user's real keys. Phase 1 should add a per-install secret, a Host/Origin check, and a warning when bound to a non-loopback address.
- **OAuth token stores are left out.** `proxy/providers/copilot_auth.rs` (2083 lines) and `codex_oauth_auth.rs` (1127 lines) keep plaintext token JSON, each with its own atomic writer (`copilot_auth.rs:1247-1294`). There is no 0600 equivalent on Windows and no keychain. They are missing from the catalog, the writer snapshots and the `fs::write` lint (they use `OpenOptions`, so the lint won't catch them).
- **`services/provider/mod.rs` (3986 lines) is never mentioned.** The AppAdapter work only names `live.rs`, `commands/provider.rs` and `commands/config.rs`.
- **`codex_history_migration.rs` (2630 lines) writes into Codex's own state DB.** It changes `threads.model_provider` (lines 644, 1157) based on `CC_SWITCH_CODEX_MODEL_PROVIDER_ID`. The Codex adapter and takeover refactor must keep that id and its `settings.json` migration markers stable. The "byte-stable snapshots" non-goal does not cover `~/.codex` SQLite.
- **The sync protocol is not covered.** `services/sync_protocol.rs:27-28` (`DB_COMPAT_VERSION=6`, `LEGACY=5`) is part of the S3 path (`s3_sync.rs:221-227`). The plan never says when a schema change must bump it.
- **Profiles and frontend storage are not in the per-app state work.** The `profiles` table (`schema.rs:314`, `tests/profile_roundtrip.rs`) and app-id-keyed localStorage (`App.tsx` `STORAGE_KEY`, `useCodexCommonConfig`, `useGeminiCommonConfig`) need to come along when app slugs or aliases change.

**(b) Sequencing mistakes**
- **Bug-report capture comes before its taps.** The Phase 2 `CaptureTap` needs the tap and stream-framing layer, which is built in Phase 3. Either move capture to Phase 3 or build it as a temporary hook on today's `response_processor.rs`.
- **Phase 1 changes command signatures before bindings exist.** `launch_session_terminal` and the `PickedPath` tokens change in Phase 1, but the bindings and the "inventory callers through the bindings" mitigation arrive in Phase 2. Per-domain `AppManifest::commands` permission lists also fit better after the Phase 5 `collect_commands!` split.
- **The v16 tombstone migration lands before the migration framework.** It is written in Phase 1 the old way (in the `user_version` loop at `schema.rs:430`), then has to be rewritten in Phase 4. It also ships before the sync-compatibility decision (`DB_COMPAT_VERSION`) and before any downgrade story exists.
- **The Phase 0 DB fixture range is wrong.** `v{0..15}.sql` treats v0 as SQLite. Pre-SQLite users upgrade from `config.json` through `database/migration.rs` (`migrate_from_json`), which has no fixture. Separately, Phase 4 "drops the `MultiAppConfig` dependency" from MCP import, which could break that legacy path unless it is pinned first.

**(c) Claims the code contradicts**
- **`atomic_write` already preserves file mode** on Unix (`config.rs:323-330`), so the planned "mode-preserving variant" already exists. The real gaps are that a new file gets umask permissions (for example 0644 on a fresh `auth.json`) and there is no fsync before the rename.
- **Hermes is not "hard-coded false."** `ProxyTakeoverStatus` (`proxy/types.rs:112-119`) has no hermes field at all. Only opencode and openclaw are hard-coded false (`services/proxy.rs:714-716`).
- **Spend limits don't "fail open"; they aren't enforced at all.** `check_provider_limits` is only called from the UI command (`commands/usage.rs:257`). The proxy never checks it. Fixing `unwrap_or(0.0)` (`usage_stats.rs:1648,1667`) is a UI fix only. Enforcement (refuse or fail over at the limit) is a missing feature.
- **`#[allow(dead_code)]` in `proxy/` is 69 today**, not 72.
- **No CSP tightening is actually scheduled.** `tauri.conf.json:29` allows `connect-src https: http:` and `img-src http:`. That is needed for user base URLs and usage scripts, so "tighten where feasible" with no exit criterion amounts to no change.

**(d) Compatibility gaps for existing users' configs and databases**
- **Downgrade blocks startup.** `stored_user_version_exceeds_supported` (`database/mod.rs:170`) makes an older app refuse a newer DB. Once v3.19 writes v16, rolling back or switching update channel bricks startup. The plan needs a documented restore-from-pre-migration-backup flow, or forward-compatible additive migrations.
- **Mixed-version sync is a problem from Phase 1, not just Phase 4.** SQL dumps carry `PRAGMA user_version` (`backup.rs:398`). An older device importing a newer dump, or the reverse, needs a defined rule from Phase 1 onward.
- **Backups under WAL can be stale.** `backup_database_file` and pre-migration backups copy the main file, so they need a checkpoint first or the SQLite backup API. Detecting "non-local paths" (iCloud or Dropbox directories set via `set_app_config_dir_override`) is not specified.
- **Pre-migration backup only runs when `version > 0`** (`database/mod.rs:131`). v0 and JSON-era upgrades get no backup.
- **Keyed per-app settings maps need dual-write.** Reading the old flat keys covers upgrades. Without also writing them, a downgraded app loses current providers, visible apps and directory overrides.
- **Pricing provenance can't be inferred from today's data.** Existing `model_pricing` rows don't mark user edits versus seeded rows. The catalog swap could overwrite user prices or resurrect deleted ones unless rows that differ from the seed are classed as user-owned.
- **The `superseded_by` backfill and the join-table rebuilds run at startup.** On large `proxy_request_logs` that is a long blocking step. It needs to run in chunks or in the background with progress, and be resumable after a crash.