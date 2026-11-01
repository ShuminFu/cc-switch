# Per-app config adapters & MCP sync — findings (CFG-xx)

Part of the [next-phase proposals](../README.md). 48 findings. Priorities: P2 = 31, P3 = 17.

## Area summary

The eight per-app adapters are functionally rich and the newest ones (OpenClaw with a round-trip JSON5 AST, Hermes with backups + write lock + duplicate-key healing, Codex with `toml_edit`) show the team already cares about preserving user files. The problem is that this care is uneven: Claude/Codex/Gemini live writes take no backup and are fully overwritten from the DB, OpenCode is read as JSON5 but written through a key-sorting `serde_json` writer (comments and order vanish), Gemini silently replaces an unparsable `settings.json` with `{}`-plus-provider, and `config::atomic_write` is neither crash-safe (no fsync) nor atomic on Windows (remove-then-rename). Cross-cutting logic is copy-pasted per app: `write_live_snapshot`/`read_live_settings`/backfill are 8-arm `match`es in `services/provider/live.rs`, the MCP sync module has three byte-identical `collect_enabled_servers` and six near-identical import loops, and `McpApps` is a struct of six booleans that every file enumerates by hand. Security-relevant gaps are concentrated in `usage_script.rs` (QuickJS runs with no CPU/memory limit on the async runtime, scripts evaluated twice, unbounded response bodies) and in MCP ingestion (UI/deeplink specs skip `validate_server_spec`; Windows `cmd /c` wrapping re-interprets shell metacharacters; custom MCP field values are logged at info level). The suggested direction is a single `AppAdapter` trait over a format-preserving `LiveDoc` layer with shared transactional writes/backups and a fixture-driven conformance suite that every adapter must pass; `codex_history_migration.rs` should move behind a `Migration` registry with a retirement policy, and `provider.rs` should be split into per-concern meta structs. With that foundation, the requested features (diff/preview before write, import-from-live for every app, schema validation, and new tools such as Zed/Qwen Code/Continue) become additive work instead of eight more copies.

## Index

| ID | Title | Type | Priority | Effort | Confidence | Verdict |
|---|---|---|---|---|---|---|
| [CFG-01](#cfg-01) | Usage-script JS runtime has no CPU/memory limits and blocks the async runtime | security | P2 (first pass P1) | M | high | confirmed |
| [CFG-02](#cfg-02) | Gemini `settings.json` is silently replaced when it cannot be parsed | bug | P2 (first pass P1) | S | high | partially-confirmed |
| [CFG-03](#cfg-03) | Claude/Codex/Gemini live files are overwritten with no backup and backfill is silently skipped when live is unreadable | bug | P2 (first pass P1) | M | high | confirmed |
| [CFG-04](#cfg-04) | `atomic_write` is not crash-safe and not atomic on Windows; secret files inherit umask permissions | bug | P2 (first pass P1) | S | medium | partially-confirmed |
| [CFG-05](#cfg-05) | MCP specs from the UI and deeplinks bypass `validate_server_spec`; Windows `cmd /c` wrapping re-interprets shell metacharacters | security | P2 (first pass P1) | S | medium | partially-confirmed |
| [CFG-06](#cfg-06) | Introduce an `AppAdapter` trait, a shared live-file layer, and a conformance test-suite (large refactor) | refactor | P2 (first pass P1) | XL | high | partially-confirmed |
| [CFG-07](#cfg-07) | OpenCode config is read as JSON5 but written through the key-sorting `serde_json` writer (comments and ordering lost) | bug | P2 | M | high |  |
| [CFG-08](#cfg-08) | Hermes YAML section splicing drops comments and emits LF into CRLF files; `serde_yaml` 0.9 is an archived crate | bug | P2 | L | high |  |
| [CFG-09](#cfg-09) | Build one format-preserving `LiveDoc` layer instead of five ad-hoc writers (large refactor) | refactor | P2 | L | high |  |
| [CFG-10](#cfg-10) | Isolate `codex_history_migration.rs` behind a `Migration` registry with a retirement policy | refactor | P2 | L | high |  |
| [CFG-11](#cfg-11) | `provider.rs` has become a grab-bag model (ProviderMeta with 30+ optional cross-concern fields) | tech-debt | P2 | M | high |  |
| [CFG-12](#cfg-12) | Tool-native home overrides (`CODEX_HOME`, `CLAUDE_CONFIG_DIR`, `OPENCODE_CONFIG`) are not honored, while `HERMES_HOME` is | bug | P2 | S | high |  |
| [CFG-13](#cfg-13) | Gemini `.env` parser/serializer is lossy (quotes, comments, order, trailing newline) and chmods the user's `~/.gemini` directory | bug | P2 | M | high |  |
| [CFG-14](#cfg-14) | No BOM/encoding/line-ending tolerance in the shared readers | bug | P2 | S | medium |  |
| [CFG-15](#cfg-15) | Provider switch may spawn the `codex` CLI synchronously (no timeout, console flash, `.cmd` not resolved) | performance | P2 | M | high |  |
| [CFG-16](#cfg-16) | MCP transport inference is lossy across adapters (streamable HTTP becomes SSE) | bug | P2 | S | high |  |
| [CFG-17](#cfg-17) | MCP spec schema is unchecked; Codex/Grok projection silently drops non-string `args`/`env`/`headers` | bug | P2 | S | high |  |
| [CFG-18](#cfg-18) | Hermes `set_provider` resurrects fields the user cleared in the UI | bug | P2 | S | medium |  |
| [CFG-19](#cfg-19) | Vendor/model capability knowledge is hard-coded in five unrelated files | tech-debt | P2 | M | high |  |
| [CFG-20](#cfg-20) | Secrets are projected in plaintext with default permissions and logged by the MCP converter | security | P2 | M | high |  |
| [CFG-21](#cfg-21) | Test coverage is thin exactly where data loss happens; no cross-adapter conformance tests | test | P2 | L | high |  |
| [CFG-22](#cfg-22) | Feature — diff/preview before any live write | feature | P2 | L | high |  |
| [CFG-23](#cfg-23) | Feature — "Import from live config" for every app, repeatable | feature | P2 | M | high |  |
| [CFG-24](#cfg-24) | Feature — schema validation with path-based, localized errors | feature | P2 | L | high |  |
| [CFG-25](#cfg-25) | `UniversalProvider::to_codex_provider` builds TOML by string formatting without escaping | bug | P2 | S | high |  |
| [CFG-26](#cfg-26) | No developer documentation of adapter contracts, ownership, backfill rules and backup locations | docs | P2 | M | high |  |
| [CFG-38](#cfg-38) | Default Codex switch mode overwrites ~/.codex/auth.json (ChatGPT login) with the third-party API key | risk | P2 | S | medium |  |
| [CFG-39](#cfg-39) | ~/.claude.json is rewritten by unlocked read-modify-write while Claude Code owns it | bug | P2 | M | medium |  |
| [CFG-40](#cfg-40) | MCP projection deletes same-id servers the user added directly in an app, while live import only runs on first launch | bug | P2 | M | medium |  |
| [CFG-42](#cfg-42) | Only Hermes merges on write: Codex/OpenCode/Gemini/Grok projection replaces the live entry, drops app-native fields and force-enables servers the user disabled in the app | bug | P2 | M | high |  |
| [CFG-43](#cfg-43) | Codex history migration replaces session JSONL files by rename while Codex may still be appending to them | bug | P2 | S | medium |  |
| [CFG-27](#cfg-27) | Codex two-file "atomic" write is not atomic and reads a value it never uses | bug | P3 | S | high |  |
| [CFG-28](#cfg-28) | `write_json_file` alphabetizes keys and omits the trailing newline, reordering user-visible files on every switch | bug | P3 | S | high |  |
| [CFG-29](#cfg-29) | Usage-script template variables are substituted textually into JS source | bug | P3 | S | high |  |
| [CFG-30](#cfg-30) | Gemini MCP sync injects `timeout` into every server and Windows `cmd /c` wrapping leaks back into the DB on import | bug | P3 | S | high |  |
| [CFG-31](#cfg-31) | Dead code kept alive with `#[allow(dead_code)]` | tech-debt | P3 | S | high |  |
| [CFG-32](#cfg-32) | `claude_plugin.rs` and `prompt_files.rs` bypass the shared home/atomic-write helpers | tech-debt | P3 | S | high |  |
| [CFG-33](#cfg-33) | `apply_codex_official_proxy_route` reorders `config.toml` and rewrites inline tables | bug | P3 | S | high |  |
| [CFG-34](#cfg-34) | Claude Desktop restore leaves cc-switch residue and the Windows directory pick is fuzzy | bug | P3 | S | medium |  |
| [CFG-35](#cfg-35) | Feature — adapters for additional tools once `AppAdapter` exists | feature | P3 | XL | medium |  |
| [CFG-36](#cfg-36) | Feature — editable Hermes v12+ `providers:` dict entries | feature | P3 | M | medium |  |
| [CFG-37](#cfg-37) | OpenClaw section rewrite loses in-section comments and uses two different JSON5 parsers | bug | P3 | M | high |  |
| [CFG-41](#cfg-41) | MCP projection rewrites each live file once per server (plus once per disabled server) instead of once per app | performance | P3 | M | high |  |
| [CFG-44](#cfg-44) | Codex history migration is all-or-nothing per startup and its backups have no retention or visibility | tech-debt | P3 | M | high |  |
| [CFG-45](#cfg-45) | remove_server_from_codex / remove_server_from_grokbuild swallow TOML parse errors and report success | bug | P3 | S | high |  |
| [CFG-46](#cfg-46) | MCP sync silently does nothing when an app's config directory is missing, while the DB and UI record the server as enabled | ux | P3 | S | high |  |
| [CFG-47](#cfg-47) | Claude Desktop 3P profile is written with coworkEgressAllowedHosts: ["*"] | security | P3 | S | medium |  |
| [CFG-48](#cfg-48) | write_claude_config replaces an unparseable or non-object ~/.claude/config.json with {} | bug | P3 | S | medium |  |

## Findings

### CFG-01: Usage-script JS runtime has no CPU/memory limits and blocks the async runtime

**Type:** security · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/usage_script.rs:33-96` — `Runtime::new()` + `Context::full()` are created with no `set_memory_limit`, `set_max_stack_size` or interrupt handler, and `ctx.eval(script_with_vars.clone())` runs synchronously inside `pub async fn execute_usage_script`; `src-tauri/src/usage_script.rs:114-203` — a second `Runtime` re-evaluates the entire script just to fetch `extractor`; `src-tauri/src/usage_script.rs:223-227` — the only timeout (`timeout_secs.clamp(2, 30)`) applies to the HTTP request; `src-tauri/src/usage_script.rs:262` — `resp.text().await` reads an unbounded body; `src-tauri/src/usage_script.rs:238-253` — no redirect policy on the shared client, so the same-origin check at `:506-557` only covers the first hop.
- **Problem:** A script containing `while(true){}` or an exponential allocation (scripts arrive via deeplink import, `src-tauri/src/deeplink/provider.rs:246-276`, and via cloud sync) pins a tokio worker forever or OOMs the process; the `timeout` field the user configures does not cover script execution at all. Evaluating the script twice doubles cost and duplicates any top-level side effects.
- **Proposal:** Run evaluation in `spawn_blocking`; create the runtime with `set_memory_limit(64 MiB)`, `set_max_stack_size`, and `set_interrupt_handler` driven by a deadline (`timeout` setting); evaluate once and keep `request`/`extractor` in one context (or serialize the extractor result through `AsyncRuntime`); cap response bodies (e.g. 2 MiB via `Content-Length` + streamed `bytes_stream`); set `redirect::Policy::none()` (or re-validate each hop) on a dedicated client for usage requests.
- **Acceptance:** Unit tests with `while(true){}` and `new Array(1e9).fill(0)` scripts return a localized timeout/memory error within the configured timeout without blocking other Tauri commands; a 10 MB response body is rejected; a 302 to another host is refused; script top-level `globalThis.counter++` observes exactly one evaluation.
- **Verification:** **confirmed** — Verified src-tauri/src/usage_script.rs:34-57 and :115-138 create `Runtime::new()`/`Context::full()` twice with no `set_memory_limit`/`set_max_stack_size`/`set_interrupt_handler` (rg finds none anywhere in src-tauri) and run `ctx.eval` synchronously inside `pub async fn execute_usage_script` (:9), reached from `#[tauri::command] pub async fn queryProviderUsage` (src-tauri/src/commands/provider.rs:382-383) with no `spawn_blocking` or `tokio::time::timeout` on the path (services/provider/usage.rs:22-31, :179-188); the only timeout is `.timeout(timeout_secs.clamp(2, 30))` on the HTTP request at :227-240 and `resp.text()` at :262 is unbounded. The shared client built in src-tauri/src/proxy/http_client.rs:216-264 sets no `redirect` policy, so reqwest's default 10-hop following applies after the single `validate_request_url` check (:108, :506-557); reqwest strips Authorization/Cookie on cross-host hops but not custom headers such as x-api-key, and the redirect must come from the (already key-holding) provider host, so that part is minor. Deeplink import does build an enabled script with `auto_query_interval` (src-tauri/src/deeplink/provider.rs:246-276) and the frontend re-runs it on a react-query `refetchInterval` (src/lib/query/usage.ts:186), but import requires accepting `DeepLinkImportDialog` (src/App.tsx:1667), so the realistic outcome is a self-inflicted DoS (each hung call pins one tokio worker until the runtime, including the local proxy, stalls) rather than key or data loss; I downgrade to P2. The proposal is sound and unimplemented (rquickjs 0.8.1 per src-tauri/Cargo.lock:4573 exposes those limiters), with one caveat: a single-context design must keep the non-Send Runtime on the blocking thread and `block_on` the HTTP call there, which is exactly why the code drops the runtime before the `.await` today (comment at :32).

### CFG-02: Gemini `settings.json` is silently replaced when it cannot be parsed

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/gemini_config.rs:303-309` — `update_selected_type` does `serde_json::from_str::<Value>(&content).unwrap_or_else(|_| serde_json::json!({}))` and then writes the result back at `:332`; `src-tauri/src/services/provider/live.rs:1604-1608` — `write_gemini_live` merges into `read_json_file::<Value>(&settings_path).unwrap_or_else(|_| json!({}))` and writes at `:1648`; contrast `src-tauri/src/gemini_mcp.rs:14-21`, whose `read_json_value` propagates the parse error.
- **Problem:** Gemini CLI loads `settings.json` through a comment-stripping parser, so a user file containing `//` comments, a trailing comma, or a BOM fails `serde_json`, and every provider switch rewrites the whole file as `{"security":{"auth":{"selectedType":...}}}` (plus provider `config`), deleting `mcpServers`, themes, telemetry and any other user settings with no backup.
- **Proposal:** Parse with a JSONC-tolerant reader (the `json5` crate already in the tree, or `jsonc-parser`), and on any parse failure abort the write with a localized error naming the file and line/column instead of substituting `{}`; route the write through the shared backup+transaction layer (CFG-03/CFG-06).
- **Acceptance:** Fixture `settings.json` with a leading comment and `mcpServers` survives `write_gemini_live` and `write_packycode_settings` byte-for-byte except for the `security.auth.selectedType` key; an invalid file causes the switch to fail with an error and the file is untouched.
- **Verification:** **partially-confirmed** — src-tauri/src/gemini_config.rs:303-306 does `serde_json::from_str::<Value>(&content).unwrap_or_else(|_| serde_json::json!({}))` and writes it back via `write_json_file` at :332, and src-tauri/src/services/provider/live.rs:1604-1605 does the same `read_json_file::<Value>(..).unwrap_or_else(|_| json!({}))` before `write_json_file` at :1648, whereas gemini_mcp.rs:19 propagates `AppError::json`. I checked upstream Gemini CLI's settings loader: it parses with `JSON.parse(stripJsonComments(content))` and passes no trailingCommas option, so `//` comments are a genuine Gemini-valid state serde_json rejects, but a trailing comma or BOM also breaks Gemini CLI itself, so those two examples are overbroad. 'Every provider switch' is too broad for the code: when `settings_config.config` is null/absent, live.rs:1630-1631 uses `read_json_file(&settings_path)?` and aborts before any write (including the .env write at :1638/:1643); however src/components/providers/forms/ProviderForm.tsx:1328-1331 always stores `config: configObj` (an object, `{}` when empty) for Gemini, so every form-created provider does take the swallowing branch and the file is replaced. No test covers an unparsable settings.json and no backup is taken (see CFG-03); the proposal is sound (json5 0.4 is already a dependency at src-tauri/Cargo.toml:82, and merely propagating `AppError::json` already yields serde_json's line/column in the message per error.rs:24-29). P2 rather than P1: silent loss of user-only Gemini settings on a routine switch, but with a narrow precondition, and cc-switch's own MCP sync re-populates `mcpServers` from the DB.

### CFG-03: Claude/Codex/Gemini live files are overwritten with no backup and backfill is silently skipped when live is unreadable

**Type:** bug · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/services/provider/live.rs:1016-1057` — `write_live_snapshot` calls `write_json_file(&path, &settings)` for Claude and `write_codex_provider_live_with_catalog` for Codex directly; `src-tauri/src/config.rs:297-352` — `atomic_write` keeps no copy of the previous content; `src-tauri/src/services/provider/mod.rs:2636` — `if let Ok(live_config) = read_live_settings(app_type.clone())` drops the backfill on any read error and continues to `write_live_with_common_config` at `:2680`; by contrast `src-tauri/src/hermes_config.rs:363-381` and `src-tauri/src/openclaw_config.rs:385-403` create retained backups under `~/.cc-switch/backups/<app>/`.
- **Problem:** A user who hand-edited `~/.claude/settings.json` (even a single trailing comma) or `config.toml` loses all of those edits on the next switch: the read fails, nothing is backfilled, and the DB copy overwrites the file. For Claude the whole file is replaced (`sanitize_claude_settings_for_live` output), so hooks/permissions added outside cc-switch are gone with no recovery path. Backup retention logic (`effective_backup_retain_count`, `src-tauri/src/settings.rs:1077-1085`) exists but is only wired for two of eight apps.
- **Proposal:** Move backup-before-write into the shared write layer (`live_fs::write_with_backup(path, bytes, BackupPolicy)`) used by every adapter; when `read_live_settings` fails during a switch, surface a blocking localized error ("live file unreadable: …; fix or choose overwrite") rather than silently continuing, with an explicit "overwrite anyway (backup taken)" path.
- **Acceptance:** Integration test: corrupt `settings.json` → switch returns an error and the file is unchanged; valid switch produces `~/.cc-switch/backups/claude/claude_<ts>.json` with the previous content and retention pruning identical to Hermes; same for Codex (`auth.json`, `config.toml`) and Gemini (`.env`, `settings.json`).
- **Verification:** **confirmed** — Verified src-tauri/src/services/provider/live.rs:1019-1021 replaces ~/.claude/settings.json wholesale with `write_json_file(&path, &sanitize_claude_settings_for_live(..))` (no merge with the live file) and :1046-1052 goes through `write_codex_live_for_provider` (src-tauri/src/codex_config.rs:1700-1725) into `write_json_file`/`write_text_file` (codex_config.rs:244-247, :319); `atomic_write` (src-tauri/src/config.rs:297-352) retains nothing, and the only retained-backup code is `create_hermes_backup` (hermes_config.rs:363-381) and `create_openclaw_backup` (openclaw_config.rs:385-403), the sole adapter users of `effective_backup_retain_count` (settings.rs:1077-1087; database/backup.rs:338 is the DB backup). src-tauri/src/services/provider/mod.rs:2636 `if let Ok(live_config) = read_live_settings(..)` silently skips both `sync_common_config_snippet_from_live` and the backfill on any error, `read_live_settings` for Claude is a plain `read_json_file` (live.rs:1336) and Codex validates TOML (codex_config.rs:1350, :281-284), so an unparsable live file is then overwritten at mod.rs:2680 with the DB copy. The `proxy_live_backup` table (services/proxy.rs:449) is only the proxy takeover/restore snapshot, not a pre-switch backup, and README.md:301's backups directory holds only DB/Hermes/OpenClaw files, so nothing mitigates this. Impact is narrower than the wording suggests: with a parsable live file the backfill at mod.rs:2641-2655 does preserve hand edits, so loss requires the file to be unreadable at switch time (a state Claude Code/Codex themselves reject), hence P2; the shared backup-before-write plus blocking-error proposal is sound and not implemented (the missing-file case `claude.live.missing` at live.rs:1330 should stay non-blocking).

### CFG-04: `atomic_write` is not crash-safe and not atomic on Windows; secret files inherit umask permissions

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/src/config.rs:317-321` — tmp file is written and `flush()`ed but never `sync_all()`ed and the parent directory is never fsynced; `src-tauri/src/config.rs:332-342` — on Windows the target is `remove_file`d before `rename`, leaving a window with no file (and permanent loss if the rename then fails); on error paths the `.tmp.<nanos>` file is left behind; `src-tauri/src/config.rs:323-330` — permissions are copied only when a previous file exists, so a freshly created `auth.json`/`config.toml` carrying `experimental_bearer_token` (`src-tauri/src/codex_config.rs:1251-1291`) is created with the default umask; only Gemini explicitly chmods 600 (`src-tauri/src/gemini_config.rs:178-187`).
- **Problem:** A crash or power loss right after the rename can leave a zero-length `settings.json`/`config.toml` on filesystems without rename-ordering guarantees; on Windows any process holding the file (Claude Desktop, an editor, antivirus) makes `remove_file` succeed but `rename` fail, deleting the user's config outright.
- **Proposal:** `sync_all()` the tmp file, fsync the parent dir on Unix, drop the manual `remove_file` (`std::fs::rename` already uses `MOVEFILE_REPLACE_EXISTING` on Windows) and retry on sharing violations, delete the tmp file on every error path, and add `atomic_write_secret` that creates files `0o600` (used for `auth.json`, `config.toml` with bearer tokens, Claude Desktop profile, Hermes/OpenClaw files that carry `api_key`).
- **Acceptance:** Unit tests inject failures after tmp creation and after rename and assert the original file is intact and no `*.tmp.*` remains; a Windows CI test holds the target open with `FILE_SHARE_READ` and asserts the write fails without deleting the original; new secret files have mode 0600 on Unix.
- **Verification:** **partially-confirmed** — src-tauri/src/config.rs:318-320 only `write_all`+`flush` the tmp file (rg finds no `sync_all`/fsync anywhere in src-tauri), :326-329 copy permissions only when a predecessor exists, :335-337 unconditionally `let _ = fs::remove_file(path)` before `rename` on Windows, and no code path removes `<name>.tmp.<nanos>` on failure (rg '.tmp.' shows no cleanup; the same remove-then-rename pattern recurs in proxy/providers/codex_oauth_auth.rs:829-832). The comment at :334 is wrong because `std::fs::rename` on Windows uses MoveFileExW with MOVEFILE_REPLACE_EXISTING, so the manual delete only adds a loss window. The Windows failure story is overstated, though: a holder opened without FILE_SHARE_DELETE makes `remove_file` fail and the rename fail too (original intact, tmp leaked), and on Win10 1903+ NTFS DeleteFile has POSIX semantics so the rename still succeeds; loss needs a FILE_SHARE_DELETE holder (e.g. Node/libuv readers) under older semantics, or an AV grabbing the tmp file between delete and rename. The secret-permissions part holds: only gemini_config.rs:178-187 chmods `.env` to 600, while auth.json/config.toml (bearer token injected by codex_config.rs:1251-1291 and written via :319) and ~/.claude/settings.json (which carries ANTHROPIC_AUTH_TOKEN) go through `atomic_write` with umask defaults, even though a 0600 pattern already exists in settings.rs:683-688 and codex_oauth_auth.rs:808-817. Proposal is sound and not implemented; P2 for a single-user desktop app (crash window and multi-user exposure are both real but low-likelihood).

### CFG-05: MCP specs from the UI and deeplinks bypass `validate_server_spec`; Windows `cmd /c` wrapping re-interprets shell metacharacters

**Type:** security · **Priority:** P2 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/src/services/mcp.rs:19-49` — `McpService::upsert_server` saves the spec and syncs to live without calling any validator; `src-tauri/src/deeplink/mcp.rs:116-129` — new servers are built from `server_spec.clone()` straight out of the URL payload; `validate_server_spec` is only invoked on the *import-from-live* paths (`src-tauri/src/mcp/claude.rs:69`, `codex.rs:214`, `gemini.rs:65`, `grokbuild.rs:99`, `hermes.rs:293`); `src-tauri/src/claude_mcp.rs:18-60` — on Windows `npx`/`npm`/`node`/… are rewritten to `cmd /c <cmd> <args…>` with args passed verbatim, so an arg such as `foo & curl evil` or `%USERPROFILE%` is interpreted by `cmd.exe`; `src-tauri/src/mcp/validation.rs:48-91` only checks that `command`/`url` are non-empty strings.
- **Problem:** An MCP server definition is arbitrary code execution inside the user's coding tools. A `ccswitch://` link (or a cloud-synced DB) can register a server whose `type` is unknown, whose `args` contain non-strings, whose `url` is `file:`/`javascript:`, or whose arguments gain shell semantics once cc-switch wraps them for Windows — none of which is rejected or shown prominently before the live write.
- **Proposal:** Call a strict validator in `upsert_server` (and deeplink import): `type ∈ {stdio,http,sse}`, `command` string, `args` array-of-strings, `env`/`headers` object-of-strings, `url` must parse with scheme `http`/`https`; when wrapping for Windows, refuse (or quote per `cmd` rules) args containing `& | < > ^ % "`; make the deeplink confirm dialog render the resolved command line and env keys; add a `source: deeplink` tag that the live sync surfaces.
- **Acceptance:** Unit tests for the validator cover each rejected shape; a deeplink with `{"mcpServers":{"x":{"command":"npx","args":["a","&","calc"]}}}` is rejected on Windows wrapping and accepted elsewhere only with a warning; `tests/mcp_commands.rs` gains cases for `upsert_server` rejecting invalid specs before any live file changes.
- **Verification:** **partially-confirmed** — Core observation confirmed: McpService::upsert_server (src-tauri/src/services/mcp.rs:19-54) saves and syncs with no validator, the deeplink path builds `server: server_spec.clone()` (src-tauri/src/deeplink/mcp.rs:119-131), none of the sync_single_server_to_* bodies validate (src-tauri/src/mcp/claude.rs:117-134, codex.rs:349-400, gemini.rs:113-129, grokbuild.rs:132-156, hermes.rs:176-202, opencode.rs:184-198), validate_server_spec is only called on import-from-live paths (plus opencode.rs:236, which the finding missed) and lives at src-tauri/src/mcp/validation.rs:8-51 (the file is 69 lines, so the cited :48-91 is wrong), and set_mcp_servers_map (src-tauri/src/claude_mcp.rs:387-390) applies wrap_command_for_windows (claude_mcp.rs:55-59), which extends the original args verbatim after `/c <cmd>`. Mitigations the finding understates: deeplink import only happens after an explicit click (src/components/DeepLinkImportDialog.tsx:721 `<Button onClick={handleImport}>`) on a dialog that lists each server id plus `Command:`/`URL:` (src/components/deeplink/McpConfirmation.tsx:56-60) with the warning at src/i18n/locales/en.json:2403, the UI form already enforces command/url presence (src/components/mcp/McpFormModal.tsx:348-358), and deeplink servers are already tagged 'imported' (deeplink/mcp.rs:127, though claude_mcp.rs:383 strips tags before the live write); the dialog does, however, hide args/env/headers, and sync_current_to_live (src-tauri/src/services/provider/live.rs:1278-1295, called from src-tauri/src/commands/import_export.rs:67 and commands/sync_support.rs:12) does project a restored DB's servers to live without per-server review. Impact is overstated: a stdio spec is already arbitrary command execution (command 'powershell' needs no metacharacters), so cmd.exe re-interpreting `&` adds no capability, refusing `%` or `"` would break legitimate Windows configs that rely on `%APPDATA%` expansion, and unknown type / non-string args / file: URLs are robustness defects the consuming CLI rejects rather than exploits. Verdict P2: adding schema validation in upsert_server and rendering the full command line and env keys in McpConfirmation is worthwhile hardening (and src-tauri/tests/mcp_commands.rs indeed has no invalid-spec upsert case), but it is gated by user confirmation and is not an exploitable-now P1.

### CFG-06: Introduce an `AppAdapter` trait, a shared live-file layer, and a conformance test-suite (large refactor)

**Type:** refactor · **Priority:** P2 · **Effort:** XL · **Confidence:** high

- **Evidence:** Per-app dispatch is duplicated as parallel `match app_type` blocks: `src-tauri/src/services/provider/live.rs:1016-1120` (write), `:1309-1421` (read), `:825-905` (backfill), `src-tauri/src/prompt_files.rs:21-38` (prompt file), `src-tauri/src/mcp/mod.rs:23-40` (MCP exports), `src-tauri/src/services/mcp.rs:31-48` (six copy-pasted `if prev_apps.X && !server.apps.X` blocks), `src-tauri/src/app_config.rs:9-22` (`McpApps` as six booleans). `collect_enabled_servers` is byte-identical in `src-tauri/src/mcp/claude.rs:18-38`, `mcp/codex.rs:23-43`, `mcp/gemini.rs:18-38`; `should_sync_*_mcp` exists six times (`mcp/claude.rs:11-15`, `codex.rs:16-20`, `gemini.rs:11-15`, `grokbuild.rs:14-16`, `hermes.rs:47-49`, `opencode.rs:29-32`); four different rollback/safety strategies coexist: manual two-file rollback (`src-tauri/src/codex_config.rs:210-258`), snapshot-and-restore (`src-tauri/src/claude_desktop_config.rs:950-967`), lock+backup (`src-tauri/src/hermes_config.rs:105-108,423-469`), lock+backup+optimistic concurrency (`src-tauri/src/openclaw_config.rs:324-376`), and none (Claude, Gemini, OpenCode, Grok).
- **Problem:** Every behavioral fix (backups, BOM handling, validation, diff preview) must be re-implemented eight times and currently lands in a random subset; adding a ninth tool touches ~12 files; tests cannot be shared, so the same bug class (comment loss, silent overwrite) recurs per adapter.
- **Proposal:** Create `src-tauri/src/adapters/` with
  ```rust
  pub trait AppAdapter: Send + Sync {
  fn app(&self) -> AppType;
  fn mode(&self) -> WriteMode;                   // Exclusive | Additive
  fn files(&self) -> Vec<OwnedFile>;              // path + Ownership::{Whole, Section(&str), Keys(&[&str])}
  fn read_live(&self) -> Result<LiveState>;       // typed per-app Value + raw docs
  fn project(&self, p: &Provider, docs: &mut LiveDocs) -> Result<()>;   // DB -> live, format-preserving
  fn backfill(&self, docs: &LiveDocs, stored: &Value) -> Result<Value>;  // live -> DB, strips projections
  fn validate(&self, settings: &Value) -> Vec<ValidationIssue>;
  fn mcp(&self) -> Option<&dyn McpProjector>;    // to_native/from_native/sync/remove
  fn prompt_file(&self) -> Option<PathBuf>;
  fn import_candidates(&self) -> Result<Vec<ImportCandidate>>;
  }
  ```
  plus `live_fs::{Transaction, write_with_backup, atomic_write_secret, read_text_lenient}` (CFG-03/04/14) and a static registry `ADAPTERS: &[&dyn AppAdapter]`. Replace `McpApps` with `BTreeSet<AppType>` behind a compat serde layer. Add `src-tauri/tests/adapter_conformance.rs` that iterates the registry over a fixture corpus (`tests/fixtures/<app>/{pristine,commented,crlf,bom,invalid}.*`) and asserts: untouched sections are byte-identical, comments survive, invalid input never writes, backfill(project(x)) == x, MCP to_native/from_native round-trips, and backups are produced.
- **Acceptance:** `live.rs`/`mcp/mod.rs`/`prompt_files.rs` contain no `match AppType` over adapter behavior; adding a toy `AppType::Dummy` adapter in a test registers everything (switch, MCP, prompts, import) without touching other modules; the conformance suite runs in CI for all eight adapters.
- **Verification:** **partially-confirmed** — Evidence holds: `match app_type` dispatch at src-tauri/src/services/provider/live.rs:1017 (write_live_snapshot) and :1310 (read_live_settings), an `if matches!` chain in restore_live_settings_for_provider_backfill (live.rs:825-903), two matches in src-tauri/src/prompt_files.rs:21-38, six copy-pasted blocks at src-tauri/src/services/mcp.rs:31-48, six booleans at src-tauri/src/app_config.rs:9-22 (also six DB columns at src-tauri/src/database/dao/mcp.rs:90-95), `diff` shows collect_enabled_servers byte-identical across src-tauri/src/mcp/claude.rs:18-38, codex.rs:23-43 and gemini.rs:18-38, and should_sync_* exists six times (claude.rs:11, codex.rs:16, gemini.rs:11, grokbuild.rs:14, hermes.rs:47, opencode.rs:29). The four safety strategies are real: manual two-file rollback in write_codex_live_atomic (src-tauri/src/codex_config.rs:211-258), snapshot/restore with_rollback (src-tauri/src/claude_desktop_config.rs:950-967), lock+backup (src-tauri/src/hermes_config.rs:105-108 and 427-459), lock+backup+`current_source != self.original_source` optimistic check (src-tauri/src/openclaw_config.rs:324-361), while Claude/Gemini/OpenCode/Grok writes go through plain atomic_write (src-tauri/src/config.rs:297) with no backup (rg finds no backup/rollback/snapshot in gemini_config.rs, opencode_config.rs or grok_config.rs); no AppAdapter trait or registry exists (only ProviderAdapter at src-tauri/src/proxy/providers/adapter.rs:16), there is no src-tauri/tests/fixtures directory, and AppType::Hermes appears in 14 files, consistent with the ~12-files-per-new-tool claim. Two proposal details are off: McpApps is persisted as six boolean columns and exposed to the frontend as apps.claude etc., so the DAO and IPC shape, not just a compat serde layer, must change; and the acceptance `backfill(project(x)) == x` cannot hold literally for Codex, whose backfill intentionally strips mcp_servers and the unified-session bucket and re-injects modelCatalog (live.rs:867-900), so the conformance assertion must be 'equal modulo documented lossy fields'. Priority corrected to P2: the duplication is real and the direction sound, but an XL big-bang rewrite across ~13k lines fixes no user-facing defect by itself, and most of the value (shared collect_enabled_servers/should_sync, a write_with_backup helper, iterable McpApps) can be extracted incrementally first.

### CFG-07: OpenCode config is read as JSON5 but written through the key-sorting `serde_json` writer (comments and ordering lost)

**Type:** bug · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/opencode_config.rs:88-104` — `read_opencode_config` parses with `json5::from_str`; `src-tauri/src/opencode_config.rs:106-112` — `write_opencode_config` calls `write_json_file`, which at `src-tauri/src/config.rs:274-286` sorts keys recursively (`sort_json_keys`, `:257-271`) and pretty-prints with `serde_json`; every provider write, plugin toggle, and MCP sync goes through `set_provider`/`set_mcp_server`/`add_plugin` (`:123-138`, `:182-194`, `:206-249`).
- **Problem:** OpenCode's `opencode.json(c)` commonly contains comments and intentionally ordered `provider`/`agent` blocks; the first cc-switch write strips every comment and alphabetizes all keys, and because OpenCode is "additive mode" this happens on each MCP sync too, not only on switch.
- **Proposal:** Reuse the OpenClaw round-trip JSON5 document (`OpenClawConfigDocument::set_root_section`, `src-tauri/src/openclaw_config.rs:262-322`) generalized into a `Json5Doc` in the shared `LiveDoc` layer (CFG-09); write only the touched root section (`provider`, `mcp`, `plugin`), keeping the rest of the file verbatim; create new files with the `$schema` header as today.
- **Acceptance:** Fixture `opencode.jsonc` with comments and non-sorted keys passes through `set_provider` with only the `provider.<id>` subtree changed (diff-based assertion); `tests/adapter_conformance.rs` covers OpenCode.

### CFG-08: Hermes YAML section splicing drops comments and emits LF into CRLF files; `serde_yaml` 0.9 is an archived crate

**Type:** bug · **Priority:** P2 · **Effort:** L · **Confidence:** high

- **Evidence:** `src-tauri/src/hermes_config.rs:301-307` — the replaced section is regenerated by `serde_yaml::to_string`, so comments inside `custom_providers:`/`model:`/`mcp_servers:` are lost; `src-tauri/src/hermes_config.rs:267-292` + test `:1299-1316` — a section's byte range runs to the next top-level key, so comments that precede the *next* section ("# This is a comment" before `agent:`) are deleted when the previous section is replaced (`:325-345`); `:245-259` accepts `\r` when detecting keys but the serialized replacement is LF-only, and test `:1490-1512` only asserts de-duplication, so CRLF files end up with mixed line endings; `src-tauri/Cargo.toml:70` pins `serde_yaml = "0.9"` (archived upstream in 2024); YAML anchors/merge keys in the rewritten section are inlined or dropped by the re-serialization.
- **Problem:** Hermes users maintain `config.yaml` by hand (Hermes' own Web UI edits it too); every cc-switch provider save or MCP sync erodes their comments and can produce a file with mixed line endings that diff tools flag entirely.
- **Proposal:** Short term: end a section at the last non-blank, non-comment line that belongs to it (keep trailing comment blocks with the following section) and emit the detected line ending; medium term: replace text splicing + `serde_yaml` with a CST-preserving editor (`yaml-rust2`/`saphyr` with span info, or a `YamlDoc` in the `LiveDoc` layer that patches scalars in place and only re-serializes added entries).
- **Acceptance:** Fixture with comments inside and between sections, CRLF endings, and an anchor used by an untouched section survives `set_provider`/`update_mcp_servers_yaml` except for the mutated keys; `cargo deny`/`cargo audit` no longer reports `serde_yaml` as unmaintained.

### CFG-09: Build one format-preserving `LiveDoc` layer instead of five ad-hoc writers (large refactor)

**Type:** refactor · **Priority:** P2 · **Effort:** L · **Confidence:** high

- **Evidence:** JSON is written by two incompatible paths: sorted `write_json_file` (`src-tauri/src/config.rs:274-286`) vs insertion-ordered `to_string_pretty` in `src-tauri/src/claude_mcp.rs:113-120` and `src-tauri/src/gemini_mcp.rs:23-30`; OpenClaw reads with the `json5` crate (`src-tauri/src/openclaw_config.rs:206`) but writes through the `json_five` round-trip parser (`:248`), two dialect implementations that can disagree; Codex re-parses `config.toml` from text in each helper in a single switch (`src-tauri/src/codex_config.rs:928, 966, 1073, 1225, 1260, 1419, 1525, 1671, 1810` plus `validate_config_toml` at `:271-278`), passing `String` between steps; Hermes splices text (`src-tauri/src/hermes_config.rs:325-357`); Gemini `.env` is a hand-rolled parser (`src-tauri/src/gemini_config.rs:26-50`).
- **Problem:** Each writer has its own loss profile (ordering, comments, line endings, quoting style), and pipeline steps that round-trip through strings (`settings["config"] = doc.to_string()` then re-parse) hide intermediate failures and make it impossible to compute a diff for preview (CFG-22).
- **Proposal:** `src-tauri/src/live_doc/{json.rs,json5.rs,toml.rs,yaml.rs,env.rs}` exposing a common `LiveDoc` API (`get_path`, `set_path`, `remove_path`, `to_bytes` preserving style, `diff(&other)`), backed by `serde_json` with order preservation + style sniffing (indent, trailing newline, BOM, EOL), `json_five` rt AST (also used for reads), `toml_edit::DocumentMut` carried through the Codex pipeline (parse once per write), a CST YAML editor, and a line-preserving env editor. Adapters (CFG-06) operate only on `LiveDoc`s.
- **Acceptance:** Property test: for every fixture, `LiveDoc::parse(bytes).to_bytes() == bytes`; Codex switch parses `config.toml` exactly once (assert via a counter in tests); OpenClaw read and write use the same parser.

### CFG-10: Isolate `codex_history_migration.rs` behind a `Migration` registry with a retirement policy

**Type:** refactor · **Priority:** P2 · **Effort:** L · **Confidence:** high

- **Evidence:** `src-tauri/src/lib.rs:609-668` — three migrations run in a `spawn_blocking` task on every startup with log-only error handling; `src-tauri/src/codex_history_migration.rs:50-99` — a hard-coded list of ~50 legacy preset ids (`CC_SWITCH_LEGACY_CODEX_MODEL_PROVIDER_IDS`) that must track the frontend presets; `src-tauri/src/codex_state_db.rs:18` — `state_5.sqlite` hard-coded ("update this single source of truth when a new state DB version ships"); `src-tauri/src/codex_history_migration.rs:1022-1058` — the migration rewrites every `sessions/**/*.jsonl` and `archived_sessions` file in place (guarded only by an mtime/len re-check at `:1060-1073`, not a lock against a running Codex); the file is 2630 lines (1280 of tests) and is the second-largest module in the crate.
- **Problem:** One-shot data migrations are mixed with product features (`unify_codex_session_history`), live forever, and silently stop applying when Codex bumps the DB filename, while the preset-id list rots. The code also touches user session history on startup, which is the riskiest write the app performs.
- **Proposal:** Move to `src-tauri/src/migrations/{mod.rs,codex_third_party_bucket.rs,codex_template_bucket.rs,codex_official_unify.rs}` with `trait Migration { fn id(); fn introduced_in(); fn retire_after(); fn applies(&Ctx)->bool; fn run(&Ctx)->Outcome; fn rollback(); }`, a persisted ledger (already partially in settings), a startup gate that skips when a Codex process is running (or holds a lock file in `~/.codex`), `state_*.sqlite` glob discovery, and derive legacy ids from the presets JSON resource at build time. Surface outcomes in the Settings UI ("migrations" panel) instead of only logs. Schedule removal of the third-party bucket migration (retire after N minor versions once the ledger shows completion).
- **Acceptance:** `lib.rs` startup calls `migrations::run_pending(&ctx)` only; the legacy-id list is generated/validated against presets in a test; a test proves the runner skips when a sentinel "Codex running" condition holds; a `retire_after` version triggers a compile-time or test-time reminder.

### CFG-11: `provider.rs` has become a grab-bag model (ProviderMeta with 30+ optional cross-concern fields)

**Type:** tech-debt · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/provider.rs:393-516` — `ProviderMeta` mixes proxy routing (`api_format`, `prompt_cache_*`, `codex_chat_reasoning`, `impersonate_claude_code`, `max_output_tokens`, `local_proxy_request_overrides`), billing (`cost_multiplier`, `limit_*_usd`, `pricing_model_source`), partner marketing (`is_partner`, `partner_promotion_key`), Claude Desktop (`claude_desktop_mode`, `claude_desktop_model_routes`), auth (`auth_binding` plus the legacy `github_account_id` kept "for compatible read", `:512-515`, `:557-571`), usage (`usage_script`); `src-tauri/src/provider.rs:223-275` — `UsageScript` carries template-specific credentials for NewAPI, Volcengine (`access_key_id`/`secret_access_key`) and Zhipu team plans as flat optional fields; `src-tauri/src/provider.rs:647-868` — `UniversalProvider` is a parallel model with its own generators; `:885-965` — OpenCode typed structs live in the same file; `:116-118` — `resolve_usage_credentials` documents that it "mirrors the frontend getProviderCredentials", i.e. dual maintenance.
- **Problem:** Every feature adds another `Option<T>` with camelCase rename; nothing groups fields by owner, legacy fields never retire, and the serialized shape is effectively untyped for the frontend (no schema). Credential resolution is duplicated in TS and Rust.
- **Proposal:** Split into `src-tauri/src/provider/{mod.rs, meta.rs, usage.rs, universal.rs, opencode.rs}`; regroup `ProviderMeta` into `#[serde(flatten)]` sub-structs (`ProxyMeta`, `BillingMeta`, `ClaudeDesktopMeta`, `CodexMeta`, `UsageMeta`, `AuthMeta`) keeping the wire format identical; add a `meta_version` and a one-time normalizer that folds `githubAccountId` into `authBinding`; generate the TS type from Rust via `ts-rs`/`specta` so the frontend stops hand-mirroring; expose `resolve_usage_credentials` as a command and delete the TS copy.
- **Acceptance:** Serde round-trip snapshot tests (existing JSON fixtures) pass unchanged; `rg githubAccountId src-tauri/src` only hits the normalizer; the frontend imports the generated `ProviderMeta` type.

### CFG-12: Tool-native home overrides (`CODEX_HOME`, `CLAUDE_CONFIG_DIR`, `OPENCODE_CONFIG`) are not honored, while `HERMES_HOME` is

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/codex_config.rs:158-164` — `get_codex_config_dir` checks only the cc-switch settings override then `~/.codex` (`CODEX_HOME` appears only in a comment at `src-tauri/src/codex_state_db.rs:4`); `src-tauri/src/config.rs:37-43` — `get_claude_config_dir` ignores `CLAUDE_CONFIG_DIR`; `src-tauri/src/opencode_config.rs:35-47` — ignores `OPENCODE_CONFIG`/`OPENCODE_CONFIG_DIR` even though `:51-81` honors `OPENCODE_DB`/`XDG_DATA_HOME`; `src-tauri/src/hermes_config.rs:53-67` — honors `HERMES_HOME`; no login-shell environment loading exists (no `fix-path-env` or equivalent in `src-tauri/Cargo.toml`/`src-tauri/src/lib.rs`).
- **Problem:** Users who relocate their Codex/Claude config via the tools' own env vars get cc-switch writing to `~/.codex`/`~/.claude` while the CLI reads elsewhere ("switch did nothing"); inconsistent semantics across apps also mean the GUI (which does not inherit shell env on macOS/Linux launchers) behaves differently from the terminal for Hermes only.
- **Proposal:** Give every adapter the same resolution order: cc-switch override → tool-native env var (read from the login-shell environment captured once at startup, e.g. via `$SHELL -lc env`) → platform default; show the resolved path in Settings with its source.
- **Acceptance:** Unit tests per adapter for the three-tier order; Settings UI displays "resolved from CODEX_HOME"; a user with `CODEX_HOME=~/work/.codex` sees switches land there.

### CFG-13: Gemini `.env` parser/serializer is lossy (quotes, comments, order, trailing newline) and chmods the user's `~/.gemini` directory

**Type:** bug · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/gemini_config.rs:26-50` — `parse_env_file` splits on the first `=` and keeps surrounding quotes verbatim (dotenv, which Gemini CLI uses, strips matching quotes and handles `export`/escapes); `:126-140` — `serialize_env_file` sorts keys, drops every comment and unknown line, and joins with `\n` without a trailing newline; `:163-172` — `write_gemini_env_atomic` forces `0o700` on the parent directory on every write; the parsed map is what `read_live_settings` backfills into the DB (`src-tauri/src/services/provider/live.rs:1358`).
- **Problem:** `GEMINI_API_KEY="abc"` is stored and later proxied with literal quotes; users' comments and grouping in `.env` disappear after the first switch; a shared/group-readable `~/.gemini` is silently locked down.
- **Proposal:** Implement a dotenv-compatible line model (key, raw value, quoting style, inline comment) in the `LiveDoc` env editor (CFG-09): update values in place, append new keys at the end, keep comments/order, preserve quoting, end with newline; only chmod the file (0600), not the directory, unless the directory was created by cc-switch.
- **Acceptance:** Fixture `.env` with comments, quoted values and `export` prefixes round-trips through switch with only the provider keys changed; `GEMINI_API_KEY="abc"` resolves to `abc` in `resolve_usage_credentials`.

### CFG-14: No BOM/encoding/line-ending tolerance in the shared readers

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/src/config.rs:246-254` — `read_json_file` feeds `fs::read_to_string` output straight to `serde_json` (a UTF-8 BOM makes it fail with "expected value at line 1 column 1"); all adapters use `read_to_string`, which errors on non-UTF-8 bytes (e.g. GBK-edited files on Windows) with a raw io error; BOM stripping exists only in unrelated modules (`src-tauri/src/proxy/handlers.rs:1815`, `src-tauri/src/services/skill.rs:2025`, `src-tauri/src/database/backup.rs:79`); CRLF is handled only in Hermes (`src-tauri/src/hermes_config.rs:245-259`).
- **Problem:** Windows editors (Notepad, PowerShell `Set-Content`) routinely add a BOM; the result is a "file missing/invalid" switch failure or, combined with CFG-02/CFG-03, a silent overwrite.
- **Proposal:** Shared `read_text_lenient(path) -> (String, TextStyle{bom, eol, encoding})` that strips UTF-8 BOM, decodes UTF-16 BOMs, falls back to lossy decoding with a warning, and feeds `TextStyle` to the writer so BOM/EOL are preserved; localized parse errors with line/column.
- **Acceptance:** Conformance fixtures with BOM and CRLF for every adapter read correctly and are written back with the same BOM/EOL.

### CFG-15: Provider switch may spawn the `codex` CLI synchronously (no timeout, console flash, `.cmd` not resolved)

**Type:** performance · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/codex_config.rs:867-884` — template lookup order is `models_cache.json` → `codex debug models --bundled` → static resource, and it runs inside the write path (`write_codex_provider_live_with_catalog` `:1201-1213` ← `src-tauri/src/services/provider/live.rs:1046`); `:807-842` — each candidate in `codex_cli_candidates()` (`:793-805`, fixed list `:649-654` plus nvm/fnm/volta/mise scans `:706-791`) is executed with `Command::output()` and no timeout; no `creation_flags(CREATE_NO_WINDOW)` unlike `src-tauri/src/commands/misc.rs:19,239`; the bare `"codex"` candidate cannot resolve `codex.cmd` on Windows (only the `APPDATA\npm` branch at `:782-790` tries `.cmd`).
- **Problem:** For ProxyChat providers without a `models_cache.json`, every switch can take seconds (Node startup per candidate), flash a console window on Windows, hang if the CLI blocks, and on Windows PATH installs fall through to the static template anyway.
- **Proposal:** Cache the extracted template under `~/.cc-switch/cache/codex-model-template-<codex-version>.json`; probe the CLI off the switch path (background task at startup / on demand with a 5 s timeout, hidden window, `.cmd`/`.exe` resolution); prefer the bundled static template unless the cached one is newer.
- **Acceptance:** A switch with no cache file completes without spawning any process (assert via an injectable spawner in tests); Windows run shows no console flash; CLI probing has a timeout test.

### CFG-16: MCP transport inference is lossy across adapters (streamable HTTP becomes SSE)

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/mcp/hermes.rs:141-143` — any Hermes entry with `url` is imported as `type: "sse"`; `src-tauri/src/mcp/opencode.rs:152-154` — OpenCode `remote` → `sse`; `src-tauri/src/gemini_mcp.rs:60-67` — `url` → `sse`, `httpUrl` → `http`; `src-tauri/src/mcp/grokbuild.rs:68-74` — `url` → `http`; on write, Hermes/OpenCode drop `type` entirely (`src-tauri/src/mcp/hermes.rs:86-96`, `mcp/opencode.rs:78-97`).
- **Problem:** A streamable-HTTP server imported from Hermes or OpenCode is re-projected to Claude/Codex as `sse` and fails to connect; the same server imported from Grok becomes `http`. Round trips through two apps flip the transport.
- **Proposal:** Default URL-only entries to `http` (current MCP spec; SSE is deprecated), keep an optional `transportHint` captured from the source app (e.g. Hermes `transport:` field if present), and add a cross-adapter matrix test (`from_native(to_native(spec)) == spec` for all app pairs).
- **Acceptance:** Matrix test passes for stdio/http/sse specs across all six MCP projectors; importing an OpenCode `remote` server and syncing to Codex yields `type = "http"`.

### CFG-17: MCP spec schema is unchecked; Codex/Grok projection silently drops non-string `args`/`env`/`headers`

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/mcp/codex.rs:608-616` — `args.iter().filter_map(|x| x.as_str())` discards numeric/bool args (`["--port", 8080]` → `["--port"]`); `:624-634` and `:640-650` — non-string `env`/`headers` values are dropped; `src-tauri/src/mcp/opencode.rs:61-65` keeps `arg.clone()` (so apps disagree); `src-tauri/src/mcp/validation.rs:48-91` — validator never checks element types; `src-tauri/src/mcp/codex.rs:465-552` — generic converter skips nested objects/mixed arrays with only a log line.
- **Problem:** A server that works in Claude silently loses arguments in Codex/Grok, producing confusing "server failed to start" errors far from the cause.
- **Proposal:** Strict typed `McpServerSpec` (serde) validated at `upsert_server` (shares CFG-05); stringify scalar args rather than dropping; return `Vec<ProjectionWarning>` from projectors and show them in the MCP UI/sync result instead of `log::warn!`.
- **Acceptance:** Unit tests: numeric arg survives Codex/Grok projection as `"8080"`; a nested-object custom field yields a user-visible warning in `SyncResult`.

### CFG-18: Hermes `set_provider` resurrects fields the user cleared in the UI

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/src/hermes_config.rs:838-854` — on update, every key of the on-disk entry that is absent from the UI payload is copied back (`new_map.entry(k).or_insert_with(...)`); the UI sends only the fields it knows (`tests/hermes_roundtrip.rs:53-62`), so an intentionally removed `headers`/`api_mode`/`models` entry cannot be deleted through cc-switch; `src-tauri/src/hermes_config.rs:927-947` — `apply_switch_defaults` always rewrites `model.provider` and `model.default`.
- **Problem:** Forward-compat merge is correct for unknown fields but wrong for known fields the user deliberately blanked; there is no tombstone semantics, so "delete a header" is impossible without editing YAML by hand.
- **Proposal:** Treat `null` in the payload as an explicit delete and only carry over keys that are *unknown* to the form schema; alternatively diff against the snapshot the form was opened with (OpenClaw already does optimistic concurrency at `src-tauri/src/openclaw_config.rs:327-337`).
- **Acceptance:** Test: provider on disk with `headers:`; UI payload with `"headers": null` → key removed; payload omitting `rate_limit_delay` → key preserved.

### CFG-19: Vendor/model capability knowledge is hard-coded in five unrelated files

**Type:** tech-debt · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/codex_config.rs:52-64` (`CODEX_WEB_SEARCH_REJECT_HOSTS`/`_MODEL_PREFIXES`, "verified 2026-06-28 doc audit"), `:148-155` (`CODEX_RESERVED_MODEL_PROVIDER_IDS`); `src-tauri/src/model_capabilities.rs:68-105` (exact text-only model tails such as `glm-5.2`, `kat-coder-pro v1`); `src-tauri/src/claude_desktop_config.rs:35-73` (`claude-opus-4-8`, `claude-fable-5` route ids) and `:846-849` (MiMo substring detection); `src-tauri/src/codex_history_migration.rs:50-99` (legacy preset ids).
- **Problem:** Each list has its own matching rule (host substring, model prefix, exact tail, substring) and update cadence; a new vendor needs edits in several Rust files plus the frontend presets, and nothing verifies the lists agree with the shipped presets.
- **Proposal:** Single `src-tauri/src/resources/vendor_capabilities.json` (`{ hosts: [...], model_patterns: [...], capabilities: { web_search: false, image_input: false, parallel_tools: true, thinking_history_quirk: "mimo" } }`) loaded once via `include_str!`, consumed by codex catalog generation, the proxy rectifier, Claude Desktop routing, and the migration; a test asserts every preset host/model in the frontend presets resolves to an entry.
- **Acceptance:** The five lists are deleted; `rg 'minimaxi.com|xiaomimimo' src-tauri/src` hits only the resource loader; preset-coverage test passes.

### CFG-20: Secrets are projected in plaintext with default permissions and logged by the MCP converter

**Type:** security · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/mcp/codex.rs:664-673` — every non-core MCP field is logged at `info` level with its value (`"已转换自定义字段 '{key}' = {value:?}"`), so a stdio server's custom `api_key`/`token` field or a `headers` map lands in the log file; `:207` logs the same at `debug` on import; `src-tauri/src/codex_config.rs:1251-1291` — provider API keys are written into `config.toml` as `experimental_bearer_token`; `src-tauri/src/claude_desktop_config.rs:1026-1033` — the real upstream key goes into the 3P profile JSON; `src-tauri/src/hermes_config.rs:822-836` — Hermes `api_key` written literally although Hermes supports `key_env` (see `tests/hermes_roundtrip.rs:44`), and OpenCode supports `{env:VAR}` references (`src-tauri/src/provider.rs:921`); `src-tauri/src/config.rs:323-330` — new files are not created 0600.
- **Problem:** Keys spread to more files than the tools require, with weaker permissions than the tools themselves use (Codex writes `auth.json` 0600), and can leak into logs that users attach to bug reports.
- **Proposal:** Redact values in all adapter logging (log key names only); use `atomic_write_secret` (CFG-04) for files known to carry credentials; offer a per-app "reference env var instead of inline key" projection where the tool supports it (`key_env`, `{env:…}`, Grok `env_key`).
- **Acceptance:** `rg 'value:\?' src-tauri/src/mcp` returns nothing; log capture test shows no header/env values; files listed above are 0600 after a fresh switch.

### CFG-21: Test coverage is thin exactly where data loss happens; no cross-adapter conformance tests

**Type:** test · **Priority:** P2 · **Effort:** L · **Confidence:** high

- **Evidence:** `#[test]` counts: `src-tauri/src/mcp/claude.rs` 0, `mcp/codex.rs` 0, `mcp/gemini.rs` 0, `mcp/validation.rs` 0, `opencode_config.rs` 0, `gemini_mcp.rs` 0, `claude_plugin.rs` 0, `prompt_files.rs` 0, `openclaw_config.rs` 5, `usage_script.rs` 3 (vs 64 in `codex_config.rs`); integration tests: `src-tauri/tests/hermes_roundtrip.rs:35-124` is the only round-trip test for an adapter, `tests/app_type_parse.rs` has 2 trivial tests, `tests/app_config_load.rs:138-231` only covers v1 rejection; no test anywhere asserts comment/ordering/BOM/CRLF preservation or "invalid live file → no write" (grep of `src-tauri/tests` for comment/preserve only finds token/env preservation cases).
- **Problem:** The regressions reported by users (#3633 CRLF duplicate sections, lost `mcpServers`) are in untested paths; each adapter's safety net is whatever its author remembered.
- **Proposal:** `src-tauri/tests/adapter_conformance.rs` + `tests/fixtures/<app>/…` (see CFG-06) run against the adapter registry; targeted unit tests for the MCP projectors (`to_native`/`from_native` matrices), `usage_script` limits, `atomic_write` failure injection; add a `cargo test` job that runs the conformance suite on all three OSes (CI matrix already exists in `.github/workflows/ci.yml:62`).
- **Acceptance:** Every adapter has ≥1 fixture per category (pristine, commented, CRLF, BOM, invalid); MCP module line coverage ≥80%; the suite fails on the known-bad behaviors listed in CFG-02/03/07/08/13 until fixed.

### CFG-22: Feature — diff/preview before any live write

**Type:** feature · **Priority:** P2 · **Effort:** L · **Confidence:** high

- **Evidence:** `src-tauri/src/services/provider/mod.rs:2626-2680` — `switch_normal` backfills then immediately calls `write_live_with_common_config`; `SwitchResult` only carries `warnings` (`:2660-2663`); the Hermes/OpenClaw write outcomes return `backup_path` after the fact (`src-tauri/src/hermes_config.rs:114-120`, `src-tauri/src/openclaw_config.rs:78-86`); no "plan" API exists in `src/lib/api` (grep for diff/preview finds only workspace previews).
- **Problem:** Users cannot see what cc-switch is about to change in files they also edit by hand (especially additive apps and proxy takeover), and support cannot ask "what did it write?".
- **Proposal:** Add `plan_switch(app, provider_id) -> Vec<FileChange { path, before, after, unified_diff, ownership }>` built from `LiveDoc::diff` (CFG-09), an optional "confirm before writing" setting, and a post-write "changes applied" panel with the backup path; reuse for MCP sync and takeover enable/disable.
- **Acceptance:** Switching with the setting on shows a per-file diff dialog; cancel leaves files untouched; the plan output is what the write step applies (assert `apply(plan) == write` in tests).

### CFG-23: Feature — "Import from live config" for every app, repeatable

**Type:** feature · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/services/provider/live.rs:1427-1440` — `import_default_config` returns `Ok(false)` for additive apps and whenever any non-seed provider exists; `:1338-1342` — Claude Desktop `read_live_settings` is unsupported; dedicated importers exist only for OpenCode/OpenClaw/Hermes (`:1688`, `:1763`, `:1851`); Grok has `read_grok_live_settings` (`src-tauri/src/grok_config.rs:308-321`) but no importer; Codex live import collapses to a single provider even when `config.toml` defines several `[model_providers.*]` tables.
- **Problem:** Users with existing multi-provider setups (several Codex `model_providers`, Claude settings with a different key than the DB) cannot bring them into cc-switch after the first run, and Claude Desktop users have no import at all.
- **Proposal:** `AppAdapter::import_candidates()` returning `(name, settings_config, fingerprint)` per detected provider (Codex: one per `model_providers.*`; Claude: current env; Gemini: `.env`; Claude Desktop: current profile's gateway); a UI flow with dedupe by credential fingerprint and "already imported" badges; available at any time, not only when the list is empty.
- **Acceptance:** A `config.toml` with three provider tables yields three candidates; re-running import is idempotent; Claude Desktop 3P profile imports as a Direct-mode provider.

### CFG-24: Feature — schema validation with path-based, localized errors

**Type:** feature · **Priority:** P2 · **Effort:** L · **Confidence:** high

- **Evidence:** Claude settings are only coerced by `sanitize_claude_settings_for_live` (`src-tauri/src/services/provider/live.rs:166`); Gemini validation checks only that `env`/`config` are objects (`src-tauri/src/gemini_config.rs:224-276`); OpenCode writes `"$schema": "https://opencode.ai/config.json"` (`src-tauri/src/opencode_config.rs:92-94`) but never validates against it; Grok's `validate_config_toml` (`src-tauri/src/grok_config.rs:63-138`) is the one adapter with field-level checks; several errors are Chinese-only strings (`src-tauri/src/hermes_config.rs:163`, `src-tauri/src/openclaw_config.rs:207,268-270`) while others use `AppError::localized`.
- **Problem:** Typos (`ANTHROPIC_BASE_URl`, `wire_api = "response"`) are written to live files and only surface as runtime failures in the tool; error text is inconsistent across languages.
- **Proposal:** Bundle JSON Schemas (OpenCode config, Gemini CLI settings, Claude Code settings, Hermes/OpenClaw subsets maintained in-repo) and a TOML-table schema for Codex/Grok; `AppAdapter::validate` returns `Vec<ValidationIssue{path, key, params}>` rendered by the editor with inline markers; all adapter errors go through `AppError::localized`.
- **Acceptance:** Saving a Codex provider with `wire_api = "response"` shows an inline error at `model_providers.custom.wire_api`; `rg 'AppError::Config\(format!' src-tauri/src/*_config.rs` returns no user-facing messages.

### CFG-25: `UniversalProvider::to_codex_provider` builds TOML by string formatting without escaping

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/provider.rs:797-809` — `format!(r#"model = "{model}" … base_url = "{codex_base_url}""#)` interpolates user-controlled `model`, `reasoning_effort` and `base_url`; the path is live (`src-tauri/src/commands/provider.rs:779-818`, `src-tauri/src/database/dao/universal_providers.rs`).
- **Problem:** A model id or URL containing `"` or `\` (e.g. a Windows-style path or a quoted display name) produces invalid TOML that later fails `validate_config_toml` at switch time with an unrelated error, or injects extra keys.
- **Proposal:** Build the document with `toml_edit::DocumentMut`/`toml_edit::value()`; add a test with `model = r#"a"b"#`.
- **Acceptance:** Generated config parses for any string input; the injection test passes.

### CFG-26: No developer documentation of adapter contracts, ownership, backfill rules and backup locations

**Type:** docs · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `docs/` contains only user guides (`docs/guides/codex-*-guide-*.md`, `docs/user-manual`); `src-tauri/src/codex_config.rs:1` starts with `use` (no module doc) although it defines the ownership sentinels (`:15-32`) and the three-way auth/config write routing (`:1692-1725`); backfill stripping rules are spread across `src-tauri/src/services/provider/live.rs:767-905` and `src-tauri/src/codex_config.rs:1632-1690`; backup roots differ per feature (`~/.cc-switch/backups/{hermes,openclaw,codex-history-provider-migration-v1,codex-official-history-unify-v1}`, `src-tauri/src/hermes_config.rs:364`, `openclaw_config.rs:386`, `codex_history_migration.rs:318-322,749-754`).
- **Problem:** Contributors cannot tell which keys cc-switch owns in each live file, what is stripped on backfill, or where to look when a user reports a lost file.
- **Proposal:** `docs/dev/config-adapters.md` with a per-app ownership matrix (file, format, mode, owned keys/sections, backfill strips, backup path, env overrides), the SSOT model, and sequence diagrams for switch/backfill/takeover; rustdoc module headers for each adapter; a user-facing "Where are my backups?" page.
- **Acceptance:** Doc exists and is linked from `CLAUDE.md`/`README`; each adapter module has a `//!` header pointing to its row in the matrix.

### CFG-38: Default Codex switch mode overwrites ~/.codex/auth.json (ChatGPT login) with the third-party API key

**Type:** risk · **Priority:** P2 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/src/settings.rs:521` — `preserve_codex_official_auth_on_switch: false` is the default. `src-tauri/src/codex_config.rs:1715-1724` — `should_write_auth` is true for every non-official provider while that flag is false, so `write_codex_live_for_provider` takes the `write_codex_live_atomic(auth, config_text)` branch; `src-tauri/src/codex_config.rs:244` — `write_json_file(&auth_path, auth)` replaces the whole file. The config-only path (`prepare_codex_provider_live_config` → `experimental_bearer_token`, `:1733-1744`) already exists but is opt-in. `src-tauri/src/services/provider/mod.rs:2636-2663` — the only copy of the previous tokens is the backfill into the *current* provider's DB snapshot (skipped when `read_live_settings` fails). `src-tauri/src/codex_config.rs:1715-1718` + `:361-386` — switching back to an official provider whose stored `auth` has no login material takes the config-only branch and leaves the third-party key in `auth.json`.
- **Problem:** Out of the box, every switch to a third-party Codex provider destroys the user's ChatGPT OAuth material (`tokens.*`, `last_refresh`) on disk; it survives only inside cc-switch's SQLite DB. A `codex login` performed while a third-party provider is active is captured into that third-party provider's snapshot on the next switch, never into the official one. If the official provider was seeded without login material, switching back leaves `auth.json` holding the third-party key while `config.toml` points at OpenAI (401 until the user logs in again). The safe mode is hidden behind an off-by-default 'compat' flag with no explanation in the switch flow.
- **Proposal:** Default `preserve_codex_official_auth_on_switch` to `true` for new installs (honour an explicit `false` in existing settings files). In the `should_write_auth` branch, refuse to overwrite an `auth.json` that has OAuth material (`codex_auth_has_oauth_login_material`) with an API-key-only payload unless the user confirmed once. When the official provider's stored `auth` has no login material, do not leave a third-party key behind: strip `OPENAI_API_KEY` from `auth.json` instead. Surface the mode in the Codex settings card with a one-line explanation.
- **Acceptance:** Fresh install with a logged-in Codex: switching official → third-party → official with default settings leaves `~/.codex/auth.json` byte-identical (tokens intact). Unit test for `write_codex_live_for_provider` with an OAuth `auth.json` on disk and a third-party provider asserts the OAuth keys survive. Switching third-party → official with an empty stored `auth` never leaves `OPENAI_API_KEY=<third-party key>` in `auth.json`.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### CFG-39: ~/.claude.json is rewritten by unlocked read-modify-write while Claude Code owns it

**Type:** bug · **Priority:** P2 · **Effort:** M · **Confidence:** medium

- **Evidence:** `src-tauri/src/claude_mcp.rs:345-404` — `set_mcp_servers_map` reads the whole `~/.claude.json` (`:349-353`), replaces `mcpServers`, and writes the entire document back (`:402`) with no lock and no check that the file is unchanged since the read; the same pattern is in `set_has_completed_onboarding` (`:150-173`) and `upsert_mcp_server` (`:239-266`), and it is executed once per MCP server by `sync_single_server_to_claude`/`remove_server_from_claude` (`src-tauri/src/mcp/claude.rs:117-149`) inside `project_servers_to_app` (`src-tauri/src/services/mcp.rs:234-241`). The codebase already has the right guard elsewhere: `src-tauri/src/openclaw_config.rs:327-337` refuses to save when the on-disk source changed since load, and `src-tauri/src/codex_history_migration.rs:1060-1073` re-checks mtime/len before replacing a file.
- **Problem:** `~/.claude.json` is Claude Code's live state file (projects, OAuth account, onboarding counters, tips, per-project MCP/allow lists) that Claude Code rewrites at startup, on exit and on many events. Every cc-switch window 'read → serialize → tmp write → rename' (repeated N times per sync) can clobber a Claude Code write with a stale copy (silent loss of Claude Code state), or be clobbered by Claude Code right after the rename (silent loss of the MCP change the UI just reported as applied).
- **Proposal:** Add a shared `guarded_rmw(path, |value| …)` helper for every JSON adapter that edits a foreign-owned file: capture `(mtime, len)` or a content hash at read, re-check immediately before `rename`, retry a few times on mismatch, otherwise return a typed conflict error. Coalesce the per-server loop into one RMW per app (see the batching finding). Optionally take an advisory `flock` on Unix.
- **Acceptance:** A test simulates an external modification of `~/.claude.json` between read and write and asserts the external change is preserved (retry) or a conflict error is returned, never silently overwritten; `project_servers_to_app` performs at most one rewrite of `~/.claude.json` per call.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### CFG-40: MCP projection deletes same-id servers the user added directly in an app, while live import only runs on first launch

**Type:** bug · **Priority:** P2 · **Effort:** M · **Confidence:** medium

- **Evidence:** `src-tauri/src/services/mcp.rs:225-243` — `project_servers_to_app` calls `remove_server_from_app` for every DB server whose `apps.<app>` flag is off, regardless of whether cc-switch ever wrote that entry. The removers delete by id only: `src-tauri/src/mcp/codex.rs:404-444`, `src-tauri/src/mcp/claude.rs:137-149`, `src-tauri/src/mcp/gemini.rs:132-144`, `src-tauri/src/mcp/grokbuild.rs:158-181`, `src-tauri/src/mcp/hermes.rs:236-246`, `src-tauri/src/opencode_config.rs:196-204`. `src-tauri/src/lib.rs:760-761` — live MCP configs are imported into the DB only when the table is empty, so an entry added later with `claude mcp add` or by hand in `config.toml` is unknown to the DB (contrast `src-tauri/src/lib.rs:677-707`, where OpenCode/OpenClaw/Hermes *providers* are re-imported on every startup). The projection runs after every provider switch (`src-tauri/src/services/provider/mod.rs:2743`), on `sync_current_to_live` (`src-tauri/src/services/provider/live.rs:1224,1295`) and on the manual sync.
- **Problem:** A user with `context7` in the DB enabled only for Claude who later adds a `context7` entry directly to `~/.codex/config.toml` loses that Codex entry on the next provider switch (for any app) or sync, with no warning; the same applies to `~/.claude.json`, Gemini `settings.json`, `opencode.json`, `~/.grok/config.toml` and Hermes `config.yaml`. The DB is treated as the single source of truth for *removal* although it is populated from the live files exactly once.
- **Proposal:** Track ownership: when cc-switch projects a server into an app, record `(app, id)` (a `projected_apps` column on `mcp_servers` or a sidecar table); `project_servers_to_app` removes only ids it previously projected whose live entry still equals the last projected spec, and reports foreign entries as warnings instead of deleting them. Alternatively (or additionally) run the MCP import on every startup like the additive-provider import, so hand-added entries become DB rows before any projection.
- **Acceptance:** Test: DB has server X with `apps.codex=false`; `~/.codex/config.toml` contains a hand-written `[mcp_servers.X]`; after `sync_enabled_for_app(Codex)` the entry is still present and the result carries a warning naming it. A server cc-switch itself projected and then disabled is still removed.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### CFG-42: Only Hermes merges on write: Codex/OpenCode/Gemini/Grok projection replaces the live entry, drops app-native fields and force-enables servers the user disabled in the app

**Type:** bug · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/mcp/opencode.rs:76` and `:96` — `convert_to_opencode_format` unconditionally inserts `"enabled": true`; `src-tauri/src/opencode_config.rs:182-194` — `set_mcp_server` replaces the entry wholesale, so OpenCode-only keys (`timeout`, OAuth settings, `enabled: false`) are lost. `src-tauri/src/mcp/codex.rs:393` — `doc["mcp_servers"][id] = Item::Table(...)` replaces the Codex table, so hand-set `enabled = false`, `startup_timeout_sec`, `enabled_tools` disappear unless they happen to be in the DB spec; `src-tauri/src/mcp/grokbuild.rs:154` and `src-tauri/src/mcp/gemini.rs:124-128` do the same. By contrast `src-tauri/src/mcp/hermes.rs:188-233` merges and lets an existing Hermes `enabled` win (`:225-227`) over a hard-coded allowlist (`HERMES_EXTRA_FIELDS`, `:32-40`), so for Hermes a cc-switch 'enable' is a silent no-op when the user had `enabled: false`, and any Hermes field outside the allowlist is dropped.
- **Problem:** Toggling a server in cc-switch (or any provider switch, which re-projects) overrides per-app decisions the user made inside the app: an OpenCode server the user disabled comes back enabled, Codex per-server timeouts/tool filters vanish, while in Hermes the opposite happens (cc-switch cannot enable a server). Behaviour differs per app with no documentation, and the Hermes allowlist needs manual upkeep as Hermes evolves.
- **Proposal:** Define one merge policy in the shared layer: core transport keys (`type/command/args/env/url/headers`, per adapter mapping) come from the DB; every other key already present in the live entry is preserved (inverse allowlist, no per-app field lists); `enabled` is written explicitly from cc-switch's toggle for all apps. Implement it once as `merge_live_entry(existing, projected, core_keys)` and use it in the Codex, OpenCode, Gemini and Grok writers; move Hermes onto the same helper.
- **Acceptance:** Round-trip tests per adapter: a live entry with an app-native extra key and `enabled:false` → cc-switch enable → the extra key survives and `enabled` is true; cc-switch disable → entry removed/disabled consistently across apps; a Hermes entry with a key not in the old allowlist survives a sync.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### CFG-43: Codex history migration replaces session JSONL files by rename while Codex may still be appending to them

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/src/codex_history_migration.rs:1022-1058` — `rewrite_codex_session_file_lines` reads the whole file, rewrites the `session_meta` line and calls `atomic_write` (`:1056`), i.e. tmp file + `rename` over the original (`src-tauri/src/config.rs:344-350`). `:1060-1073` — `ensure_codex_session_file_unchanged` only compares mtime/len before and after the read; it cannot see an open writer. `:961-1009` — every `.jsonl` under `sessions/` and `archived_sessions/` is a candidate. `src-tauri/src/lib.rs:609-668` — the migrations run in `spawn_blocking` on every startup with no check that a `codex` process is running.
- **Problem:** On macOS/Linux a Codex process that has the rollout file open keeps writing to the now-unlinked inode: every turn after the migration is lost for that session (the file cc-switch left behind stops growing) and the state DB points at a truncated transcript. The first cc-switch start after an upgrade is exactly when a session created under an old provider id is most likely to be live. On Windows the replace typically fails with a sharing violation instead, aborting the whole run (see the all-or-nothing finding).
- **Proposal:** Skip files whose mtime is within a recent window (e.g. 15 min), log them, and do not write the completion marker while any file was skipped so the next startup retries; optionally detect a running `codex` process and defer the whole migration. If a file is skipped repeatedly, fall back to an in-place rewrite of the same inode (open with truncate) rather than rename.
- **Acceptance:** A test creates a jsonl, keeps it open with a writer, runs the migration and asserts the file was skipped and the marker not written; a second run after the writer closes migrates it and writes the marker. No session file with mtime inside the window is rewritten in one run.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### CFG-27: Codex two-file "atomic" write is not atomic and reads a value it never uses

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/codex_config.rs:210-258` — `write_codex_live_atomic` writes `auth.json` then `config.toml`, rolling back only `auth.json` if the second write fails; `_old_config` (`:228-232`) is read and discarded; a crash between the two writes leaves auth/config from different providers; `write_codex_live_config_atomic` (`:308-320`) is a plain single write.
- **Problem:** A half-applied switch routes a third-party provider's `config.toml` with the previous provider's `auth.json` (or vice versa), which is hard to diagnose.
- **Proposal:** Replace with the shared `Transaction` (CFG-06): stage all tmp files, then rename in order, journaling the previous bytes so a rollback (or next-start recovery) can restore both; delete the unused read.
- **Acceptance:** Failure-injection test between the two renames leaves both files consistent after recovery.

### CFG-28: `write_json_file` alphabetizes keys and omits the trailing newline, reordering user-visible files on every switch

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/config.rs:256-286` — recursive `sort_json_keys` before `to_string_pretty` (no final `\n`); used for `~/.claude/settings.json` (`src-tauri/src/services/provider/live.rs:1019-1021`), `~/.gemini/settings.json` (`:1648`), Claude Desktop `claude_desktop_config.json` which also holds the user's Desktop `mcpServers` (`src-tauri/src/claude_desktop_config.rs:1096-1108`); the same crate writes `~/.claude.json` unsorted (`src-tauri/src/claude_mcp.rs:113-120`) and `claude_plugin.rs` appends `\n` manually (`src-tauri/src/claude_plugin.rs:81-83`).
- **Problem:** Users who version their dotfiles get whole-file diffs after each switch; `serde_json` is already built with `preserve_order`, so the sort only loses information.
- **Proposal:** Preserve the existing key order (merge into the parsed `LiveDoc`), sort only when creating a brand-new file, detect indent/newline style and reproduce it.
- **Acceptance:** Round-trip test: a settings file with custom key order and 4-space indent is written back with identical order/indent and a trailing newline.

### CFG-29: Usage-script template variables are substituted textually into JS source

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/usage_script.rs:401-420` — `{{apiKey}}`, `{{baseUrl}}`, `{{accessToken}}`, `{{userId}}` are `str::replace`d into the script before `eval`; a value containing `"`, `\`, a newline, or `</script>`-style sequences breaks the string literal or changes program structure.
- **Problem:** Keys/tokens with special characters (JWT access tokens with `=`/`+` are fine, but base64url with `-` and user ids with quotes are not) produce confusing "parse config failed" errors, and the pattern invites injection if any value is ever attacker-influenced.
- **Proposal:** Expose values via a frozen global (`ctx.globals().set("__ccs", obj)`) and rewrite `{{apiKey}}` placeholders to `__ccs.apiKey` with a JS-string-escaped fallback for legacy scripts.
- **Acceptance:** Test with `api_key = r#"a"b\c"#` runs the bundled templates successfully.

### CFG-30: Gemini MCP sync injects `timeout` into every server and Windows `cmd /c` wrapping leaks back into the DB on import

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/gemini_mcp.rs:128-153` — `timeout` is always inserted (`max(10000, 60000)` when the source had none); `src-tauri/src/claude_mcp.rs:18-60` wraps `npx …` as `cmd /c npx …` on write, but `src-tauri/src/mcp/claude.rs:51-114` imports `~/.claude.json` verbatim, so the wrapped form (`command: "cmd"`) becomes the canonical DB spec and is then projected to Gemini/Codex/OpenCode (and to WSL/macOS via sync) as `cmd`.
- **Problem:** Re-importing from Gemini pollutes every other app with a `timeout` field; importing from Claude on Windows breaks the same server on non-Windows targets.
- **Proposal:** Emit `timeout` only when the source carried a timeout; unwrap `cmd /c X args` on import and keep wrapping purely as a Claude-on-Windows projection detail.
- **Acceptance:** Import→sync round trip on Windows yields `command: "npx"` in the DB and in `~/.gemini/settings.json`; Gemini import of a server without timeouts yields a spec without `timeout`.

### CFG-31: Dead code kept alive with `#[allow(dead_code)]`

**Type:** tech-debt · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/codex_config.rs:180-208` (`get_codex_provider_paths`, `delete_codex_provider_config`), `src-tauri/src/config.rs:223-243` (`sanitize_provider_name`, `get_provider_config_path`), `src-tauri/src/gemini_config.rs:73-123` (`parse_env_file_strict`, "reserved for future"), `src-tauri/src/provider_defaults.rs:6,13,186` (entire module unused), `src-tauri/src/services/provider/live.rs:944-960` (`LiveSnapshot` + `restore`), `src-tauri/src/opencode_config.rs:83-86`, `src-tauri/src/openclaw_config.rs:164-170`.
- **Problem:** Pre-SSOT per-provider file helpers mislead readers about the current design and dilute `cargo clippy` signal.
- **Proposal:** Delete, or wire `parse_env_file_strict` into CFG-24 validation; then forbid `allow(dead_code)` in adapter modules via clippy config.
- **Acceptance:** `rg 'allow\(dead_code\)' src-tauri/src/*_config.rs src-tauri/src/provider_defaults.rs` is empty; build is warning-free.

### CFG-32: `claude_plugin.rs` and `prompt_files.rs` bypass the shared home/atomic-write helpers

**Type:** tech-debt · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/claude_plugin.rs:14` uses `dirs::home_dir()` directly (ignores `CC_SWITCH_TEST_HOME` from `src-tauri/src/config.rs:22-34`); `:83` and `:117` use non-atomic `fs::write`; `src-tauri/src/prompt_files.rs:50` also calls `dirs::home_dir()`.
- **Problem:** Tests touching these paths can escape the temp home, and a crash mid-write truncates `~/.claude/config.json`.
- **Proposal:** Use `get_home_dir()`/`get_claude_config_dir()` and `write_json_file`/`atomic_write`; fold into the adapter layer.
- **Acceptance:** `rg 'dirs::home_dir' src-tauri/src` hits only `config.rs`; the plugin write goes through `atomic_write`.

### CFG-33: `apply_codex_official_proxy_route` reorders `config.toml` and rewrites inline tables

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/codex_config.rs:1428-1453` — `model_providers` is removed from the document and re-inserted at the end, and `into_table()` converts a user's inline `model_providers = { … }` into standard `[model_providers.*]` tables (test `:2035-2056` asserts the normalized output); `src-tauri/src/codex_config.rs:1825-1827` — `update_codex_toml_field` creates an explicit `[model_providers]` header table.
- **Problem:** Enabling takeover rewrites the shape and position of user sections, so disabling it does not restore the original text even when the content is equivalent.
- **Proposal:** Edit in place via `get_mut`/`as_table_like_mut` (keeping inline vs standard form), only create tables as implicit, and add a byte-level round-trip test enable→disable.
- **Acceptance:** `remove_codex_official_proxy_route(apply_codex_official_proxy_route(x)) == x` for fixtures with inline tables and comments.

### CFG-34: Claude Desktop restore leaves cc-switch residue and the Windows directory pick is fuzzy

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/src/claude_desktop_config.rs:1008-1019` — "restore official" writes `deploymentMode: "1p"` into both config files and prunes known keys rather than restoring the pre-apply content; `:1096-1108` — `write_deployment_mode` rewrites the user's `claude_desktop_config.json` (which also holds Desktop `mcpServers`) through the sorting `write_json_file`; `:1254-1277` — on Windows, if `Claude-3p` is absent, the first directory whose name starts with `Claude` and contains `-3p` is chosen (e.g. `Claude-3p-backup`).
- **Problem:** Users cannot get back to the exact pre-cc-switch state, and a stale/backup directory can be written to instead of the real one.
- **Proposal:** Persist a snapshot of the four files at first apply (DB or `~/.cc-switch/backups/claude-desktop/`) and restore it verbatim; match the directory exactly or let the user pick in Settings.
- **Acceptance:** Apply→restore round trip reproduces the original bytes of all four files; the Windows picker has a test rejecting `Claude-3p-old` when `Claude-3p` is missing.

### CFG-35: Feature — adapters for additional tools once `AppAdapter` exists

**Type:** feature · **Priority:** P3 · **Effort:** XL · **Confidence:** medium

- **Evidence:** `src-tauri/src/app_config.rs:370-384` — `AppType` is a closed enum with eight variants and `is_additive_mode` (`:404-409`) special-cases three; each new app today touches `live.rs`, `mcp/*`, `prompt_files.rs`, `McpApps`, `services/mcp.rs`, the deeplink parser and the frontend.
- **Problem:** Demand for more tools cannot be met cheaply until CFG-06 lands; candidates with formats cc-switch already handles are low-hanging fruit.
- **Proposal:** After CFG-06, add adapters in order of format similarity: Qwen Code (`~/.qwen/settings.json` + `.env`, same shape as Gemini), Zed (`~/.config/zed/settings.json` JSONC `language_models`), Continue (`~/.continue/config.yaml`, YAML like Hermes), Aider (`~/.aider.conf.yml` + `.env`), GitHub Copilot CLI / Crush / Goose (JSON/YAML), Cursor/Windsurf (MCP-only via `~/.cursor/mcp.json`, `~/.codeium/windsurf/mcp_config.json`), Kiro (`~/.kiro/settings/mcp.json`), Cline (MCP via `cline_mcp_settings.json`). Each ships with a conformance fixture set.
- **Acceptance:** Each new adapter is implemented as one module + registry entry + fixtures, with no edits to `live.rs`/`mcp/mod.rs`.

### CFG-36: Feature — editable Hermes v12+ `providers:` dict entries

**Type:** feature · **Priority:** P3 · **Effort:** M · **Confidence:** medium

- **Evidence:** `src-tauri/src/hermes_config.rs:601-606,726-772` — dict-sourced providers are surfaced read-only (`_cc_source = "providers_dict"`) and `set_provider`/`remove_provider` refuse them with "edit via Hermes Web UI".
- **Problem:** Users migrating to the newer Hermes schema lose cc-switch editing for exactly the providers Hermes now recommends.
- **Proposal:** Implement `providers:` dict writes with the same section-splice/CST path (CFG-08), keeping `custom_providers` precedence semantics from upstream, and let the user choose the target shape on create.
- **Acceptance:** A dict provider can be edited and removed from cc-switch; round-trip tests cover dict+list coexistence.

### CFG-37: OpenClaw section rewrite loses in-section comments and uses two different JSON5 parsers

**Type:** bug · **Priority:** P3 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/openclaw_config.rs:292-298` — an existing root key's value is replaced wholesale by `value_to_rt_value`, which re-serializes through `serde_json` (`:489-504`), so comments inside `models:`/`agents:` are lost (the test at `:971` only guarantees *top-level* comments); reads use `json5::from_str` (`:206`) while writes parse with `json_five::rt` (`:248`), and `:490-492` notes a known `json-five` 0.3.1 panic worked around by serializing with `serde_json`.
- **Problem:** The best adapter in the tree still degrades files on every provider write, and dialect differences between the two parsers can make a file readable but unwritable ("Failed to parse OpenClaw config as round-trip JSON5 document").
- **Proposal:** Use the `json_five` rt AST for reading as well (convert to `Value` from the AST), patch nested paths in the AST instead of replacing root sections, and track the upstream panic fix (or vendor a patch).
- **Acceptance:** Fixture with comments inside `models.providers` keeps them after `set_provider`; a single parser is used for read and write.

### CFG-41: MCP projection rewrites each live file once per server (plus once per disabled server) instead of once per app

**Type:** performance · **Priority:** P3 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/services/mcp.rs:234-241` — the loop calls `sync_server_to_app`/`remove_server_from_app` per server, and each adapter does a full read-parse-write: `src-tauri/src/mcp/claude.rs:126-133` and `:142-148` (`remove` writes even when the id was absent), `src-tauri/src/mcp/gemini.rs:122-128,138-144`, `src-tauri/src/mcp/codex.rs:349-399,427-442`, `src-tauri/src/mcp/grokbuild.rs:142-156,174-180`, `src-tauri/src/mcp/opencode.rs:184-207` → `src-tauri/src/opencode_config.rs:182-204`. `src-tauri/src/services/mcp.rs:196-215` — `sync_all_enabled` repeats this for all six apps. Only Hermes short-circuits unchanged writes (`src-tauri/src/hermes_config.rs:445-447`). Each write is a tmp-file + rename (`src-tauri/src/config.rs:297-352`), and Gemini re-runs the timeout/`cmd /c` transforms on every entry (`src-tauri/src/gemini_mcp.rs:76-165`).
- **Problem:** With N servers, a provider switch or 'sync all' performs up to 6·N file replacements (N of them no-ops), each a crash/race window on files other processes own (see the `~/.claude.json` race), each bumping mtime on files Claude Code and Gemini watch, and each re-emitting the whole document. Partial failure is also hard to reason about: server k failing leaves 1..k-1 applied and k+1..N not.
- **Proposal:** Give every adapter one `reconcile(desired: &IndexMap<String, Value>) -> Result<ReconcileReport>` that reads once, diffs against the current map, and writes once only if something changed; `project_servers_to_app` builds the desired map and calls it. Keep `sync_single_server_to_*` as thin wrappers over `reconcile` for the toggle path.
- **Acceptance:** A test with 20 servers and one app asserts exactly one write (count `atomic_write` calls or compare mtime) for `sync_enabled_for_app`, and zero writes when the live file already matches the desired projection.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### CFG-44: Codex history migration is all-or-nothing per startup and its backups have no retention or visibility

**Type:** tech-debt · **Priority:** P3 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/codex_history_migration.rs:143-160` — a fresh timestamped `backup_root` (`:749-754`) is created on every attempt, `migrate_codex_jsonl_files(...)?` aborts on the first file error and the completion marker is written only afterwards (`:151`); `:973-979` — each file's result is propagated with `?`; `:1031` — a single non-UTF-8 or unreadable `.jsonl` (`fs::read_to_string`) fails the whole migration, which re-runs and re-fails on every startup (`src-tauri/src/lib.rs:611-640` only logs). `:1178-1187` — every rewritten session file is copied under `~/.cc-switch/backups/codex-history-provider-migration-v1/<ts>/jsonl/…`. Retention helpers exist for Hermes/OpenClaw/DB backups (`src-tauri/src/hermes_config.rs:383-413`, `src-tauri/src/openclaw_config.rs:405-435`, `src-tauri/src/database/backup.rs:337`) but none for the two migration directories; the unify backups additionally serve as the restore ledger (`:435-465`).
- **Problem:** One bad file blocks the migration for every other session indefinitely with only a log line, while a user with a large `~/.codex/sessions` tree receives a full copy of every rewritten transcript (potentially gigabytes) under `~/.cc-switch/backups` that nothing prunes and no UI shows; each failed attempt adds another timestamp directory.
- **Proposal:** Make per-file failures non-fatal: collect `(path, error)` into the outcome, migrate the rest, write the marker with a `pending_files` list and retry only those (capped). Add a retention policy for `codex-history-provider-migration-v1` (not a ledger) and a ledger-aware one for `codex-official-history-unify-v1` (keep generations referenced by `collect_official_ledger`), plus a size readout and purge action in Settings → Backups.
- **Acceptance:** A test with one invalid-UTF-8 jsonl among valid ones migrates the valid ones, records the failure and writes the marker; backup generations beyond the retain count are pruned; the settings page shows the migration backup size.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### CFG-45: remove_server_from_codex / remove_server_from_grokbuild swallow TOML parse errors and report success

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/mcp/codex.rs:418-424` — on `parse::<DocumentMut>()` failure the remover logs a warning and `return Ok(())`; `src-tauri/src/mcp/grokbuild.rs:167-173` — same. The add path returns an error for the same condition (`src-tauri/src/mcp/codex.rs:365-369`). `src-tauri/src/services/mcp.rs:78-92` — `toggle_app` has already persisted `apps.codex=false` before calling the remover, so the UI shows the server disabled while `config.toml` still lists it; `sync_all_enabled` (`:196-215`) aggregates adapter errors, but a swallowed error never reaches it.
- **Problem:** A user disables or deletes an MCP server, sees success, and Codex/Grok keep launching the server (possibly with credentials the user meant to revoke). Because the DB now says 'not enabled', later projections only retry the same silent branch, so the state never self-heals.
- **Proposal:** Return `Err(AppError::McpValidation(…))` from both removers on parse failure (mirroring `sync_single_server_to_codex`), let `toggle_app`/`delete_server` surface it, and keep the DB change (the user's intent) while the toast says which app needs a manual fix.
- **Acceptance:** Unit test: invalid `config.toml` + `remove_server_from_codex` → `Err`; `toggle_app(…, false)` propagates the error to the command layer and the frontend shows it.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### CFG-46: MCP sync silently does nothing when an app's config directory is missing, while the DB and UI record the server as enabled

**Type:** ux · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/mcp/claude.rs:11-15` with `:42-44`, `:122-124`, `:138-140`; `src-tauri/src/mcp/codex.rs:16-20`; `src-tauri/src/mcp/gemini.rs:11-15`; `src-tauri/src/mcp/grokbuild.rs:14-16`; `src-tauri/src/mcp/opencode.rs:29-32`; `src-tauri/src/mcp/hermes.rs:47-49` — every `should_sync_*` guard makes the adapter return `Ok(())` when the directory does not exist. `src-tauri/src/services/mcp.rs:78-92` — `toggle_app` saves `apps.<app>=true` first and treats `Ok(())` as applied. The checks resolve through the override dirs (`src-tauri/src/config.rs:37-43`, `src-tauri/src/codex_config.rs:158-164`), so a mistyped custom directory in Settings produces the same permanent silent no-op.
- **Problem:** The toggle turns green, nothing is written, and the user has no signal why Claude/Codex/Gemini do not see the server. The guard's policy (never create directories for uninstalled apps) is reasonable, but its outcome is indistinguishable from success in the command result.
- **Proposal:** Return a small `SyncOutcome { applied: bool, skipped_reason: Option<&'static str> }` from the adapters (or a warning list from `toggle_app`/`upsert_server`/`sync_all_enabled`) and render a non-blocking hint in the MCP panel ('Codex config directory not found: ~/.codex — nothing was written'); offer 'create directory and write anyway' for custom override paths.
- **Acceptance:** Toggling a server for an app whose directory is missing returns a result the frontend shows as a warning; the MCP list shows a 'not applied' badge for that app; no directory is created unless the user opts in.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### CFG-47: Claude Desktop 3P profile is written with coworkEgressAllowedHosts: ["*"]

**Type:** security · **Priority:** P3 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/src/claude_desktop_config.rs:1021-1041` — `build_gateway_profile` hard-codes `"coworkEgressAllowedHosts": ["*"]` (`:1027`) for both direct and proxy profiles; tests `:1507` and `:1568` lock the value in; the profile is written to `configLibrary/<PROFILE_ID>.json` by `apply_provider_to_paths_inner` (`:969-1006`, write at `:1002`) with no UI control.
- **Problem:** Applying any cc-switch provider to Claude Desktop also lifts the Cowork egress allowlist to every host, a security-relevant change the user did not ask for and cannot see or narrow in cc-switch. A direct provider only needs the gateway host; proxy mode only needs the local proxy origin.
- **Proposal:** Default the allowlist to the minimum the profile needs (gateway host for direct, proxy origin for proxy) and expose an optional 'additional egress hosts' field in the Claude Desktop provider form, with `*` as an explicit opt-in that shows a warning.
- **Acceptance:** A direct-mode profile contains only the gateway host in `coworkEgressAllowedHosts`; proxy mode contains only the proxy origin; `*` appears only when the user enables it; existing tests are updated accordingly.
- **Verification:** added by completeness-critic pass (not independently re-verified)

### CFG-48: write_claude_config replaces an unparseable or non-object ~/.claude/config.json with {}

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/src/claude_plugin.rs:57-63` — any parse failure or non-object root collapses to `json!({})`, then `:80-83` writes `{"primaryApiKey":"any"}` back with `fs::write`, discarding whatever the file held. The sibling helper for `~/.claude.json` errors instead (`src-tauri/src/claude_mcp.rs:158-160`, '根必须是对象'). This is a second instance of the pattern already reported for Gemini `settings.json` (CFG-02), in a different adapter and trigger (plugin apply).
- **Problem:** A temporarily invalid or hand-edited `~/.claude/config.json` (trailing comma, BOM, comment) is silently wiped when the user clicks 'apply plugin config'; no backup, no error.
- **Proposal:** Mirror `set_has_completed_onboarding`: return `AppError::Config` on parse failure or non-object root, leave the file untouched, and route the write through `atomic_write` (the non-atomic `fs::write` is already noted in CFG-32).
- **Acceptance:** Unit test: invalid JSON in `~/.claude/config.json` → `write_claude_config` returns `Err` and the file is byte-identical afterwards; `clear_claude_config` behaves the same.
- **Verification:** added by completeness-critic pass (not independently re-verified)
