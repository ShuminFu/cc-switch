# Backend services — findings (SVC-xx)

Part of the [next-phase proposals](../README.md). 41 findings. Priorities: P0 = 2, P1 = 10, P2 = 20, P3 = 9.

## Area summary

The services layer (~37k lines, 36 files) is where cc-switch's hardest invariant lives: the SQLite SSOT, the per-app "live" files, and the proxy-takeover backup slot must stay mutually consistent, and the code shows real care about this (common-config strip/merge symmetry in `provider/live.rs`, takeover ownership signals, OMO snapshot/rollback in `omo.rs`, hardened zip restore in `webdav_sync/archive.rs`, a transport-agnostic `sync_protocol.rs`). The main risks are (1) multi-step mutations that persist pointers before the live write succeeds, so a single failed write can later backfill the wrong provider's credentials into the DB; (2) cloud sync that is pure last-writer-wins with no precondition on the remote manifest, so two auto-syncing devices silently lose updates, and the uploaded `db.sql` carries every API key in plaintext; (3) `futures::executor::block_on` sprinkled through synchronous Tauri commands; and (4) one zip-extraction path that skipped the hardening the other two paths have. Structurally the layer has outgrown its shape: 178 `match app_type` arms are spread across eight files, `proxy.rs` is 7.2k lines returning `Result<_, String>`, the WebDAV/S3 pairs are near-verbatim copies, and the four session-usage importers each re-implement the same insert/dedup/pricing block. Tests are strong for Codex/Claude switch semantics and proxy takeover but absent for sync end-to-end, skill install/zip paths, universal providers, sort order, env manager, and failure injection. Suggested direction for the next phase: first close the P0/P1 consistency and security holes with small targeted changes plus failure-injection tests, then invest in three refactors that pay for themselves (an `AppAdapter` trait, a `SyncTransport` trait with conflict detection, and a shared usage-ingest pipeline), and standardize on `AppError` + `tracing` so partial failures become visible instead of being logged as `let _ =`.

## Index

| ID | Title | Type | Priority | Effort | Confidence |
|---|---|---|---|---|---|
| [SVC-01](#svc-01) | Provider switch persists "current" pointers before the live write, and a failed write poisons the next backfill | bug | P0 | M | medium |
| [SVC-02](#svc-02) | Cloud sync is last-writer-wins: no remote precondition on upload and no local-dirty check on download | bug | P0 | L | high |
| [SVC-03](#svc-03) | GitHub skill archive extraction skips `enclosed_name()` and has no entry/size caps (zip-slip / zip-bomb) | security | P1 | S | medium |
| [SVC-04](#svc-04) | Sync uploads the full providers table (API keys, OAuth tokens) in plaintext | security | P1 | L | high |
| [SVC-05](#svc-05) | `futures::executor::block_on` inside synchronous Tauri commands blocks the main thread | bug | P1 | M | medium |
| [SVC-06](#svc-06) | Best-effort takeover swallows live-write errors while the DB marks the app as taken over | bug | P1 | S | medium |
| [SVC-07](#svc-07) | Skill install/update write directly into the SSOT path; a mid-way failure leaves orphan or deleted skills | bug | P1 | S | high |
| [SVC-08](#svc-08) | Gemini `settings.json` merge swallows parse errors and overwrites the file with provider-only content | bug | P1 | S | high |
| [SVC-18](#svc-18) | Introduce an `AppAdapter` trait and move per-app live/takeover/backfill logic out of the services | refactor | P1 | XL | high |
| [SVC-19](#svc-19) | Unify WebDAV and S3 behind a `SyncTransport` trait and a single auto-sync worker | refactor | P1 | L | high |
| [SVC-24](#svc-24) | No end-to-end tests for the sync protocol or auto-sync worker | test | P1 | M | high |
| [SVC-25](#svc-25) | No failure-injection tests for provider mutations, and several services have zero tests | test | P1 | M | high |
| [SVC-09](#svc-09) | Historical cost backfill uses different cache semantics for Grok Build than the calculator and SQL helpers | bug | P2 | S | high |
| [SVC-10](#svc-10) | Env manager rewrites shell rc files non-atomically and restores values unquoted | bug | P2 | S | high |
| [SVC-11](#svc-11) | Multi-row saves run as N independent writes with no transaction, and some deletes are silently ignored | bug | P2 | S | high |
| [SVC-12](#svc-12) | MCP upsert/toggle mutate the DB first, then touch up to six live files sequentially, with per-server full rewrites | bug | P2 | M | high |
| [SVC-13](#svc-13) | Session-usage import does per-row dedup queries and autocommit inserts while holding the connection mutex | performance | P2 | M | medium |
| [SVC-14](#svc-14) | Usage read queries mutate rows (cost backfill) during pagination | bug | P2 | S | high |
| [SVC-15](#svc-15) | S3 signer panics on header values built from user-provided endpoint, and sync settings do not validate path segments | security | P2 | S | medium |
| [SVC-16](#svc-16) | `ProfileService::apply` unconditionally restores live files via `disable_takeover_for_app_sync` and flattens all errors into strings | bug | P2 | S | medium |
| [SVC-17](#svc-17) | `check_updates` downloads every repository archive on each check, sequentially | performance | P2 | M | high |
| [SVC-20](#svc-20) | Collapse the four session-usage importers into a `SessionSource` trait plus one `UsageIngest` writer | refactor | P2 | L | high |
| [SVC-21](#svc-21) | Split `proxy.rs` (7.2k lines) into takeover submodules and convert its `Result<_, String>` API to `AppError` | refactor | P2 | L | high |
| [SVC-22](#svc-22) | Three error idioms coexist (`anyhow`, `String`, `AppError`) and context is lost at every boundary | tech-debt | P2 | M | high |
| [SVC-23](#svc-23) | No structured observability for the critical state machines | tech-debt | P2 | M | high |
| [SVC-26](#svc-26) | Test fixtures are duplicated and env-mutating across three places | test | P2 | S | high |
| [SVC-27](#svc-27) | Sync has no dry-run/diff: `fetch_remote_info` returns only manifest metadata | feature | P2 | M | high |
| [SVC-28](#svc-28) | Expose typed warnings and a "live ownership" query so the UI can render conflict/ownership state | feature | P2 | M | medium |
| [SVC-29](#svc-29) | No scheduled database backups or retention; only the legacy `config.json` has a backup rotation | feature | P2 | M | high |
| [SVC-30](#svc-30) | Balance/coding-plan/subscription quota clients are 16 hand-rolled HTTP functions with no shared abstraction or mocked tests | refactor | P2 | M | high |
| [SVC-31](#svc-31) | `ConfigService::sync_current_providers_to_live` is legacy dead code kept alive only by tests | tech-debt | P2 | S | high |
| [SVC-41](#svc-41) | The live/backfill/takeover ownership model is documented only in scattered Chinese inline comments | docs | P2 | S | high |
| [SVC-32](#svc-32) | Deprecated v3.6 compatibility APIs and dead code are still exported | tech-debt | P3 | S | high |
| [SVC-33](#svc-33) | `McpService::import_from_*` is six copies of the same loop | tech-debt | P3 | S | high |
| [SVC-34](#svc-34) | Additive-app live import loops are triplicated and there is no importer for other config managers | feature | P3 | M | medium |
| [SVC-35](#svc-35) | Env-var profiles: the env tooling only detects/deletes, it cannot export a provider as a shell profile | feature | P3 | M | medium |
| [SVC-36](#svc-36) | Sync materializes whole artifacts (up to 512 MB each) in memory | performance | P3 | M | high |
| [SVC-37](#svc-37) | `sync_all_providers_to_live` reports success when every provider failed to write | bug | P3 | S | high |
| [SVC-38](#svc-38) | `SkillService` discards errors in uninstall/migration paths with `let _ =` | bug | P3 | S | high |
| [SVC-39](#svc-39) | Prompt service writes DB and file in opposite orders and silently backfills DB content from the live file | bug | P3 | S | medium |
| [SVC-40](#svc-40) | Subscription quota refresh re-fetches a new Gemini access token on every call and embeds the CLI client secret | tech-debt | P3 | S | medium |

## Findings

### SVC-01: Provider switch persists "current" pointers before the live write, and a failed write poisons the next backfill

**Type:** bug · **Priority:** P0 · **Effort:** M · **Confidence:** medium

- **Evidence:** `src-tauri/src/services/provider/mod.rs:2626-2680` — `switch_normal` backfills the live file into `providers.get(&current_id)` (2635-2666), then writes `settings::set_current_provider` (2673) and `db.set_current_provider` (2676), and only then calls `write_live_with_common_config` (2680). `src-tauri/src/services/provider/mod.rs:2110-2133` — `add` saves the provider to the DB (2110) and only afterwards writes live (2132); a live-write failure returns `Err` with the DB already mutated.
- **Problem:** If the live write at 2680 fails (disk full, invalid TOML, permission), both pointers already name the new provider while the live file still holds the previous provider's config. The next `switch` reads `current_id` = new provider, sees `current_id != id`, reads the stale live file and saves it as the new provider's `settings_config`, so the new provider's DB record now carries the old provider's API key and base URL. There is no "which provider was last written to live" marker to detect this.
- **Proposal:** Reorder into a unit of work: build the effective settings, write live first (writers are already atomic per file via `config::atomic_write`), then commit the two pointers; on pointer-commit failure restore the previous live snapshot (the unused `LiveSnapshot::restore` at `live.rs:945-1014` is exactly this). Persist a `live_written_for: {app, provider_id, content_hash}` record in the DB and make the backfill in `switch_normal` skip (with a typed warning) when the live hash does not match the provider it is about to backfill into.
- **Acceptance:** A test that injects a failing live writer on `switch(A→B)`, then calls `switch(B→C)`, asserts B's `settings_config` is unchanged and a `backfill_skipped` warning is returned; `add` with a failing live writer leaves no provider row behind (or marks it not-current).

### SVC-02: Cloud sync is last-writer-wins: no remote precondition on upload and no local-dirty check on download

**Type:** bug · **Priority:** P0 · **Effort:** L · **Confidence:** high

- **Evidence:** `src-tauri/src/services/webdav_sync.rs:64-108` — `upload` PUTs db.sql, skills.zip and manifest unconditionally; `last_remote_etag`/`last_remote_manifest_hash` are written at `webdav_sync.rs:191-204` but never read before a PUT. `src-tauri/src/services/webdav_sync.rs:110-160` and `src-tauri/src/services/s3_sync.rs:93-131` — `download` calls `apply_snapshot` without checking whether the local DB changed since the last sync. `src-tauri/src/services/webdav_auto_sync.rs:150-165` and `s3_auto_sync.rs:150-165` — auto-sync fires an unconditional `upload` 1–10 s after any providers/mcp/skills/settings change.
- **Problem:** Two devices with auto-sync enabled overwrite each other's remote snapshot; the loser's edits are gone the next time the other device downloads. A manual download also discards unsynced local edits with no warning. The protocol already has the ingredients (manifest `snapshot_id`, ETag) but does not use them.
- **Proposal:** Before `upload`, fetch the remote manifest (HEAD/GET) and compare its hash/ETag to `status.last_remote_manifest_hash`; on mismatch return a typed `SyncConflict { remote_device, remote_created_at, local_dirty }` instead of uploading (WebDAV: `If-Match` on the manifest PUT; S3: compare ETag, or `If-None-Match` where supported). Track a local `dirty_since_last_sync` flag (set from the existing DB change hook, cleared in `persist_sync_success`). Make `download` refuse when dirty unless `force`. Surface the conflict to the UI (see SVC-28).
- **Acceptance:** Integration test with an in-memory transport (SVC-19) where device A and device B both upload after the same base snapshot: the second upload returns `SyncConflict`; download on a dirty local returns a conflict; forced paths still work. Auto-sync logs/persists the conflict in `status.last_error` with `last_error_source = "conflict"`.

### SVC-03: GitHub skill archive extraction skips `enclosed_name()` and has no entry/size caps (zip-slip / zip-bomb)

**Type:** security · **Priority:** P1 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/src/services/skill.rs:2238-2312` — `download_and_extract` iterates `archive.by_index(i)`, uses `file.name()` (2276), strips the root prefix and does `dest.join(relative_path)` (2289) with no `enclosed_name()` check, no entry-count cap and no total-bytes cap. Contrast `src-tauri/src/services/skill.rs:2697` (`extract_local_zip` uses `enclosed_name()`) and `src-tauri/src/services/webdav_sync/archive.rs:18,100-130` (`MAX_EXTRACT_ENTRIES`, `copy_entry_with_total_limit`).
- **Problem:** An entry named `root/../../.zshrc` or an absolute path would be written outside the temp dir. GitHub-generated archives from git trees normally cannot contain `..` components, which is why this is P1 not P0, but the URL is built from unvalidated `SkillRepo.owner/name` (`skill.rs:2768-2781` `add_repo` does no validation) and the HTTP client may traverse a user-configured proxy. A crafted archive can also exhaust disk with no size limit.
- **Proposal:** Use `entry.enclosed_name()` in `download_and_extract`, reject absolute/`..` names, apply the same `MAX_EXTRACT_ENTRIES` and total-bytes limit as `archive.rs`, and validate `owner`/`name` in `add_repo` (`^[A-Za-z0-9_.-]+$`). Factor one `extract_zip_safely(archive, dest, limits)` used by all three call sites.
- **Acceptance:** Unit tests feeding a hand-built zip with `../escape.txt`, an absolute path, 20k entries, and a 1 GB-decompressed bomb into `download_and_extract` all fail cleanly with no file written outside `dest`.

### SVC-04: Sync uploads the full providers table (API keys, OAuth tokens) in plaintext

**Type:** security · **Priority:** P1 · **Effort:** L · **Confidence:** high

- **Evidence:** `src-tauri/src/services/sync_protocol.rs:105-150` — `build_local_snapshot` dumps the DB via `export_sql_string_for_sync`; `src-tauri/src/database/backup.rs:19-25` — `SYNC_SKIP_TABLES` excludes only logs/health/rollups, so `providers` (with `settings_config` containing `ANTHROPIC_AUTH_TOKEN`, `OPENAI_API_KEY`, Codex `auth.json` login material) and `settings` are included. `src-tauri/src/services/webdav.rs:216-241` / `src-tauri/src/services/s3.rs:365-405` PUT the bytes as-is; there is no encryption code anywhere under `services/`.
- **Problem:** Any WebDAV/S3 operator, shared bucket, or leaked credential exposes every provider secret the user manages. Users typically point this at public cloud drives (the code even special-cases Jianguoyun).
- **Proposal:** Add optional client-side encryption of the artifacts: passphrase → Argon2id → XChaCha20-Poly1305 (or `age`) with the salt/nonce in the manifest (`encryption: {scheme, salt}`) and `MAX_*` limits applied post-decryption; bump `PROTOCOL_VERSION` or add a manifest capability flag so old clients fail with a clear message. Store the passphrase in the OS keychain (the `keyring` path already exists in `subscription.rs`). Until then, at minimum redact Codex `auth.json` login material from the snapshot.
- **Acceptance:** Round-trip test encrypt→upload→download→decrypt via the in-memory transport; a manifest with `encryption` set but no passphrase yields a localized error; `fetch_remote_info` reports `encrypted: true`.

### SVC-05: `futures::executor::block_on` inside synchronous Tauri commands blocks the main thread

**Type:** bug · **Priority:** P1 · **Effort:** M · **Confidence:** medium

- **Evidence:** `src-tauri/src/services/provider/mod.rs:75,83,2299-2345,2534-2580,2775-2795` — `reapply_current_codex_official_live`, `update`, `switch`, `sync_current_provider_for_app` each call `futures::executor::block_on` on DB async methods, `proxy_service.lock_switch_for_app` (a tokio `Mutex`), `is_running` (tokio `RwLock`), `hot_switch_provider_inner`, `update_live_backup_from_provider`. `src-tauri/src/services/proxy.rs:504-508,925-953` — `set_app_handle` and `disable_takeover_for_app_sync` do the same. `src-tauri/src/commands/provider.rs:103-110` — `switch_provider` is a plain (non-async) `#[tauri::command]`, which Tauri runs on the main thread.
- **Problem:** Every switch/update performs blocking file I/O plus executor-level blocking on tokio locks on the UI thread, so the window freezes for the duration; if the lock holder (e.g. `set_takeover_for_app`, which awaits `self.start()`) ever needs the main thread, the app deadlocks. `profile.rs:361` routes profile switches through the same path.
- **Proposal:** Make `ProviderService::{switch, update, add, sync_*}` and `ProfileService::apply` `async` (the DB layer already exposes async methods) and run them from `async` commands via `tauri::async_runtime::spawn_blocking` for the file-I/O parts; delete every `block_on` in services (clippy lint `disallowed_methods` for `futures::executor::block_on`). Alternatively mark the commands `#[tauri::command(async)]` as a stopgap.
- **Acceptance:** `rg block_on src-tauri/src/services` returns nothing; a clippy `disallowed-methods` entry guards it; manual check that the window remains responsive during a Codex switch with a large `config.toml`.

### SVC-06: Best-effort takeover swallows live-write errors while the DB marks the app as taken over

**Type:** bug · **Priority:** P1 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/src/services/proxy.rs:1644-1712` — `takeover_live_config_best_effort` writes Claude/Gemini/Grok live with `let _ = self.write_claude_live(..)` (1669), `let _ = self.write_gemini_live(..)` (1699), `let _ = self.write_grok_live(..)` (1705) while the Codex arm (1676-1690) propagates errors. The backup slot has already been written by `backup_live_configs` (`proxy.rs:1372-1436`), and `ProviderService::update` decides "live is proxy-owned" from `has_live_backup || live_taken_over` (`provider/mod.rs:2298-2309`).
- **Problem:** If the live write fails, the backup exists and `proxy_config.enabled` becomes true, so the app believes takeover is active and routes all subsequent provider edits into the backup only (`update_live_backup_from_provider`), while the real live file keeps pointing straight at the upstream. The user sees "proxy on" but traffic bypasses the proxy and never reaches the usage log/failover.
- **Proposal:** Propagate the write errors, collect per-app results in `takeover_live_configs`, and on failure for an app restore its backup and leave it un-taken-over (same pattern as `set_takeover_for_app` lines 800-815). Return a structured `TakeoverReport { per_app: Result }` so the frontend can show partial takeover.
- **Acceptance:** Test with a read-only Claude settings path: `start_with_takeover` reports Claude failed, no `proxy_live_backup` row for claude remains, Codex still takes over; `get_takeover_status` reflects it.

### SVC-07: Skill install/update write directly into the SSOT path; a mid-way failure leaves orphan or deleted skills

**Type:** bug · **Priority:** P1 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/services/skill.rs:644-701,762` — `install` gates on `if !dest.exists()` (644), copies straight into `ssot/<name>` (700), then saves the DB row (762). `src-tauri/src/services/skill.rs:1056-1063` — `update_skill` does `let _ = create_uninstall_backup` (1056), `fs::remove_dir_all(&dest)?` (1061), then `copy_dir_recursive(&source, &dest)?` (1063). The safe staged pattern already exists in `replace_dest_with_copy` (`skill.rs:1682-1722`).
- **Problem:** A copy failure (or DB failure after 700) leaves a partial directory at `dest`; every later install sees `dest.exists()` and skips the download, permanently installing a broken skill with no DB row. In `update_skill`, a failure after 1061 deletes the user's skill and the backup attempt was ignored.
- **Proposal:** Copy into `ssot/.<name>.tmp-<pid>-<nonce>` and `rename` into place (reuse `replace_dest_with_copy`); in `install`, if `dest.exists()` but no DB row matches, treat it as an orphan and re-stage; make the backup in `update_skill` mandatory before deletion.
- **Acceptance:** Tests injecting a failing copy in `install` and `update_skill` assert the SSOT dir is either the old content or absent, never partial, and a second `install` re-downloads.

### SVC-08: Gemini `settings.json` merge swallows parse errors and overwrites the file with provider-only content

**Type:** bug · **Priority:** P1 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/services/provider/live.rs:1595-1640` — when the provider has a `config` object, `write_gemini_live` reads the existing settings with `read_json_file::<Value>(&settings_path).unwrap_or_else(|_| json!({}))` (1605), merges the provider keys, and writes the result (1636); the other branch (1631) propagates the same read error with `?`.
- **Problem:** A transiently corrupt or half-written `~/.gemini/settings.json` (e.g. written concurrently by Gemini CLI) is silently replaced by the provider's few keys, dropping the user's `mcpServers`, theme, tool settings. The inconsistency between 1605 and 1631 shows this was not intentional.
- **Proposal:** Propagate the parse error (fail the switch with a localized message naming the file) and keep the existing behavior only when the file does not exist; optionally back the unparsable file up as `settings.json.corrupt-<ts>` before refusing.
- **Acceptance:** Test writing `{` to the Gemini settings path then switching to a provider with `config: {}` returns an error and leaves the file untouched.

### SVC-18: Introduce an `AppAdapter` trait and move per-app live/takeover/backfill logic out of the services

**Type:** refactor · **Priority:** P1 · **Effort:** XL · **Confidence:** high

- **Evidence:** 178 `AppType::X =>` match arms across the layer: `src-tauri/src/services/proxy.rs` (48: `backup_live_configs` 1372-1436 and `restore_live_config_for_app_inner` 1714-1753 are four copy-pasted blocks each; `read_/write_*_live` 2776-3010), `src-tauri/src/services/provider/mod.rs` (40: `validate_provider_settings` 3428-3556, `extract_credentials` 3557-3700, `extract_*_common_config` 3072-3312), `src-tauri/src/services/provider/live.rs` (37: `read_live_settings` 1309-1425, `import_default_config` 1427-1570, `write_live_snapshot` 1016-1100), `mcp.rs` (16), `skill.rs` (16), `profile.rs` (9), `config.rs` (8).
- **Problem:** Adding Hermes required touching eight files; app-specific knowledge (which file is live, how to detect takeover placeholders, how to strip injected keys on backfill, which keys are secrets) is scattered and inconsistent (e.g. SVC-06, SVC-08). Tests must set up every app even to test one.
- **Proposal:** `services/apps/mod.rs` with `pub trait AppAdapter { fn app(&self) -> AppType; fn mode(&self) -> Mode; fn read_live(&self) -> Result<Value>; fn write_live(&self, effective: &Value) -> Result<()>; fn live_snapshot(&self) -> Result<LiveSnapshot>; fn strip_for_backfill(&self, live: Value, stored: &Provider) -> Value; fn validate(&self, p: &Provider) -> Result<()>; fn credentials(&self, p: &Provider) -> Result<(String,String)>; fn common_config_extract(&self, v:&Value)->Result<String>; fn takeover_fields(&self, live:&mut Value, urls:&ProxyUrls, p:Option<&Provider>)->Result<()>; fn is_taken_over(&self, live:&Value)->bool; fn mcp_project(&self, servers:&[McpServer])->Result<()>; fn skills_dir(&self)->Result<PathBuf>; }` with one impl per app (`claude.rs`, `codex.rs`, `gemini.rs`, `grok.rs`, `opencode.rs`, `openclaw.rs`, `hermes.rs`, `claude_desktop.rs`) and `AppType::adapter() -> &'static dyn AppAdapter`. `ProviderService`, `ProxyService`, `McpService`, `SkillService`, `ProfileService` become app-agnostic orchestrators. Migrate one app at a time behind the trait (Codex first, it is the most tested).
- **Acceptance:** `rg "AppType::\w+ =>" src-tauri/src/services --glob '!apps/**'` is empty except `AppType::all()` iteration; adding a mock `TestApp` adapter lets `provider_service.rs` tests run without touching real app paths; all existing tests pass.

### SVC-19: Unify WebDAV and S3 behind a `SyncTransport` trait and a single auto-sync worker

**Type:** refactor · **Priority:** P1 · **Effort:** L · **Confidence:** high

- **Evidence:** `src-tauri/src/services/webdav_sync.rs:1-335` vs `src-tauri/src/services/s3_sync.rs:1-319` — `upload`, `download`, `fetch_remote_info`, `persist_sync_success`, `download_and_verify`, `run_with_sync_lock` are structurally identical except for URL/key building and the transport calls. `src-tauri/src/services/webdav_auto_sync.rs:1-274` vs `src-tauri/src/services/s3_auto_sync.rs:1-270` — byte-identical apart from type/event names (two independent `DB_CHANGE_TX` statics, two workers both woken by `database/mod.rs:87-88`). `src-tauri/src/settings.rs:1118-1144` — two status types that are the same struct.
- **Problem:** Every protocol improvement (SVC-02 preconditions, SVC-04 encryption, SVC-27 diff) must be written twice; the two auto-sync workers each upload the full snapshot on every change when both are enabled.
- **Proposal:** `pub trait SyncTransport { async fn put(&self, name:&str, bytes:Vec<u8>, ct:&str, precondition: Option<Precondition>) -> Result<Option<Etag>>; async fn get(&self, name:&str, max:usize) -> Result<Option<(Vec<u8>, Option<Etag>)>>; async fn head(&self, name:&str) -> Result<Option<Etag>>; async fn ensure_root(&self) -> Result<()>; fn describe(&self) -> String; }` implemented by `WebDavTransport` and `S3Transport` (and an `InMemoryTransport` for tests); `sync_protocol::SyncEngine<T>` owns upload/download/conflict logic; one `auto_sync` worker that iterates enabled transports. Keep the legacy-layout fallback inside the WebDAV impl.
- **Acceptance:** `webdav_sync.rs` and `s3_sync.rs` shrink to transport impls + path helpers; `webdav_auto_sync.rs`/`s3_auto_sync.rs` collapse into one module; protocol tests run once against `InMemoryTransport` and cover both.

### SVC-24: No end-to-end tests for the sync protocol or auto-sync worker

**Type:** test · **Priority:** P1 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/services/webdav_sync.rs:300-335` tests only `remote_dir_segments`; `src-tauri/src/services/s3_sync.rs:240-319` tests only key formatting/creds mapping; `src-tauri/src/services/s3.rs:868-921` live tests are gated on `S3_TEST_AK` env vars; `sync_protocol.rs:367-648` tests manifest/hash helpers but never `build_local_snapshot`/`apply_snapshot`; `webdav_auto_sync.rs:200-274` tests debounce math only, not `run_worker_loop`. No test under `src-tauri/tests/` mentions webdav, s3, `apply_snapshot` or `build_local_snapshot`.
- **Problem:** The code paths that can wipe a user's database (download → `apply_snapshot` → `import_sql_string_for_sync`) and skills SSOT (`restore_skills_zip` rename/rollback at `archive.rs:94-120`) are untested end to end; the legacy-layout fallback (`find_remote_snapshot`) and hash-mismatch rejection are untested.
- **Proposal:** After SVC-19, add `tests/sync_roundtrip.rs` using `InMemoryTransport` (and optionally `wiremock` for the WebDAV HTTP edge cases: 405/409 MKCOL, 207 PROPFIND, oversized Content-Length): upload→download round-trip preserves providers/mcp/skills and `SYNC_PRESERVE_TABLES`; corrupted artifact is rejected before `apply_snapshot`; DB import failure rolls back skills; legacy layout is read; worker loop coalesces N signals into one upload and respects `AutoSyncSuppressionGuard`.
- **Acceptance:** The listed scenarios exist and run in CI without network; coverage of `sync_protocol.rs` and `archive.rs` non-test code above 80%.

### SVC-25: No failure-injection tests for provider mutations, and several services have zero tests

**Type:** test · **Priority:** P1 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/tests/provider_service.rs` (34 tests) covers switch/delete happy paths and takeover interplay but never a failing live writer (SVC-01), `update_sort_order` (one grep hit, in `provider_commands.rs`), `remove_from_live_config`, or `add` with `add_to_live` failure; zero tests reference universal providers (`provider/mod.rs:3858-3964`), `install_from_zip`/`download_and_extract` (`skill.rs:2238-2312,2531-2672`), `env_manager`'s delete/restore (only `test_backup_dir_creation`), `PromptService` (0 unit tests), `McpService` service functions (0 unit tests; `tests/mcp_commands.rs` goes through commands), `stream_check`/`speedtest` network paths beyond pure helpers.
- **Problem:** The highest-risk partial-failure behaviors are unverified; regressions in universal/endpoint/sort features would ship unnoticed.
- **Proposal:** Introduce a `FailingFs`/`FailingWriter` hook (feature-gated `test-hooks` already exists in `commands/provider.rs:94`) or route live writes through the `AppAdapter` so a mock adapter can fail on demand; add tests for: switch with failing live write (then second switch), add with failing live write, update under takeover with failing backup update, sort-order partial failure, universal provider create/sync/delete, custom endpoint add/remove/last-used, `install_from_zip` with nested dirs and conflicts, zip-slip (SVC-03), env delete/restore round trip (SVC-10), prompt enable/disable/backfill.
- **Acceptance:** Each listed scenario has a named test; CI runs them on Linux/macOS/Windows (skill symlink paths differ per OS).

### SVC-09: Historical cost backfill uses different cache semantics for Grok Build than the calculator and SQL helpers

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/services/usage_stats.rs:1820` — `let cache_inclusive_app = matches!(log.app_type.as_str(), "codex" | "gemini");` with a comment claiming parity with `CostCalculator::calculate_for_app`; `src-tauri/src/proxy/usage/calculator.rs:62` — `matches!(app_type, "codex" | "gemini" | "grokbuild")`; `src-tauri/src/services/sql_helpers.rs:19` — `CACHE_INCLUSIVE_APP_TYPES = ["codex", "gemini", "grokbuild"]`.
- **Problem:** Grok rows that are backfilled after pricing becomes available are charged input cost on the cache-inclusive `input_tokens`, over-counting cached tokens; once a row has a cost it is never recomputed (`maybe_backfill_log_costs` skips `has_cost`).
- **Proposal:** Derive the set from one constant (`sql_helpers::CACHE_INCLUSIVE_APP_TYPES`) in all three places; add a one-off migration/backfill that recomputes grokbuild rows whose `input_token_semantics` is legacy/total.
- **Acceptance:** `test_backfill_missing_usage_costs_*` gains a grokbuild case asserting the same total as `CostCalculator::calculate_for_app("grokbuild", ..)`.

### SVC-10: Env manager rewrites shell rc files non-atomically and restores values unquoted

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/services/env_manager.rs:105-145` — `delete_single_env` filters every line whose LHS equals the var name (including inside functions/heredocs) and writes back with `fs::write(file_path, new_content.join("\n"))` (139), dropping the trailing newline. `src-tauri/src/services/env_manager.rs:200-222` — `restore_single_env` appends `format!("\nexport {}={}", name, value)` (216) with no quoting, while `env_checker.rs:140-150` stripped the quotes when capturing the value.
- **Problem:** A crash during `fs::write` truncates `~/.zshrc`; values with spaces or `$` are restored broken; the only "backup" is the JSON list of conflicts, not the file (`env_manager.rs:43-67`).
- **Proposal:** Back up the rc file itself next to the JSON, use `config::atomic_write`, comment out the line (`# cc-switch disabled: export ...`) instead of deleting it, and restore by shell-quoting (`shell-words`/`shlex`) or by un-commenting the original line.
- **Acceptance:** Tests on a temp rc file with `export ANTHROPIC_API_KEY="a b"` inside an `if` block: delete+restore round-trips byte-for-byte; a simulated write failure leaves the original file intact.

### SVC-11: Multi-row saves run as N independent writes with no transaction, and some deletes are silently ignored

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/services/provider/mod.rs:3369-3382` — `update_sort_order` calls `save_provider` per row. `src-tauri/src/services/prompt.rs:126-141` — `enable_prompt` writes the file, saves the enabled prompt, then saves every other prompt one by one. `src-tauri/src/services/provider/mod.rs:3897-3905,3931-3959` — `delete_universal` and `sync_universal_to_apps` use `let _ = state.db.delete_provider(..)`.
- **Problem:** A failure midway leaves mixed sort indexes or two prompts marked enabled; each row write also fires the DB change hook and thus a separate auto-sync signal. Swallowed deletes leave orphan `universal-*` providers that still show up in the app lists.
- **Proposal:** Add `Database::with_transaction(|tx| ..)` (there is already `unchecked_transaction` usage in `usage_stats.rs:1757`) and batch helpers `save_providers`, `save_prompts`; propagate delete errors or aggregate them into a returned warning list.
- **Acceptance:** Tests where the k-th `save_provider` fails assert no row changed; `delete_universal` with a failing child delete returns an error listing the leftover ids.

### SVC-12: MCP upsert/toggle mutate the DB first, then touch up to six live files sequentially, with per-server full rewrites

**Type:** bug · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/services/mcp.rs:19-54` — `upsert_server` saves to DB (28), then `remove_server_from_app` per de-selected app and `sync_server_to_apps` (51), each `?`-propagated. `src-tauri/src/services/mcp.rs:225-243` — `project_servers_to_app` calls `sync_server_to_app`/`remove_server_from_app` once per server. `src-tauri/src/mcp/claude.rs:117-149` — each call reads, parses and rewrites the whole `~/.claude.json`.
- **Problem:** With a broken Codex `config.toml` the Claude file is already changed and the DB already saved when the error surfaces, so the UI shows "save failed" for a half-applied change. Projecting N servers costs N read+parse+write cycles of `~/.claude.json` (which can be multi-MB because it holds project histories) on every provider switch (`provider/mod.rs:2740`).
- **Proposal:** Build the full desired `mcpServers` map per app once and write it once (`set_mcp_servers_map` already accepts the whole map); run the live projection before the DB save or collect per-app results and return a structured partial-failure report; keep `sync_all_enabled`'s aggregation semantics.
- **Acceptance:** A test with 50 servers asserts a single write of `~/.claude.json` per projection (count via a temp-dir mtime/inode or a write-counting shim); a failing Codex write leaves the Claude file and DB unchanged for `upsert_server`.

### SVC-13: Session-usage import does per-row dedup queries and autocommit inserts while holding the connection mutex

**Type:** performance · **Priority:** P2 · **Effort:** M · **Confidence:** medium

- **Evidence:** `src-tauri/src/services/session_usage.rs:182-360,410-523` — each assistant message runs `should_skip_session_insert` (an `EXISTS` by request_id plus the fingerprint query in `usage_stats.rs:372-418`) then an `INSERT`, each taking `lock_conn!` separately; no transaction (grep for `transaction` across `session_usage*.rs` only hits OpenCode test code). Same shape in `session_usage_codex.rs:386-600`, `session_usage_gemini.rs:125-216`. Runs every `SESSION_SYNC_INTERVAL_SECS` from `lib.rs:1095-1150` on a blocking thread.
- **Problem:** First import of a large `~/.claude/projects` tree issues two lookups and one autocommit write per message; SQLite autocommit fsyncs per statement, so this is minutes on HDD/Windows and competes for the single `Mutex<Connection>` with UI commands. The dedup indexes (`schema.rs:2783,2812`) help the lookups but not the commit cost.
- **Proposal:** Wrap each file in one transaction (`unchecked_transaction`), prepare the dedup/insert statements once per file, and move the pricing lookup cache (`get_model_pricing_cached`) to file scope; consider a bulk "known request_ids" set per file.
- **Acceptance:** Benchmark test importing 10k synthetic lines completes under ~1 s in-memory and performs ≤1 commit per file; existing dedup tests still pass.

### SVC-14: Usage read queries mutate rows (cost backfill) during pagination

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/services/usage_stats.rs:1466-1560` — `get_request_logs` runs a COUNT (1517), a page SELECT (1545), then `maybe_backfill_log_costs` per row which executes an `UPDATE` (1553); `get_request_detail` does the same (1566-1597). `let offset = page * page_size;` at 1524 is unchecked `u32` arithmetic.
- **Problem:** A list query is not side-effect free: it writes while holding the lock, the count and the page come from different statements (page drift under concurrent inserts), and a page far out of range can overflow and panic in debug builds.
- **Proposal:** Remove the inline backfill from read paths (startup/periodic `backfill_missing_usage_costs` at `lib.rs:1109` already covers it), compute cost on the fly for display only if still zero, use `page.checked_mul(page_size)`, and run count+page in one read transaction.
- **Acceptance:** Reads of the log tables perform no `UPDATE` (assert via `conn.changes()` or an update hook in tests); overflow page returns an empty page with `total`.

### SVC-15: S3 signer panics on header values built from user-provided endpoint, and sync settings do not validate path segments

**Type:** security · **Priority:** P2 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/src/services/s3.rs:142-147,226,380` — `headers.insert("host", host_value.parse().unwrap())` where `host_value` derives from `creds.endpoint`; `authorization.parse().unwrap()`; `content_type.parse().unwrap()`. `src-tauri/src/settings.rs:226-260` — `S3SyncSettings::validate` checks only non-empty fields; `settings.rs:166-173` trims `remote_root`/`profile` but allows `..`; `src-tauri/src/services/s3_sync.rs:212-224` builds the key with raw `format!`.
- **Problem:** An endpoint containing a non-visible-ASCII character (pasted with a trailing control char, IDN host) panics inside an async command, aborting the task so the frontend promise never settles. `profile = ".."` yields `root/v2/db-v6/../manifest.json`, writing outside the intended prefix on servers that normalize paths.
- **Proposal:** Replace `unwrap()` with `HeaderValue::from_str(..).map_err(..)`; validate endpoint host syntax and reject `.`/`..`/empty segments in `remote_root`/`profile` for both transports; reuse `webdav::path_segments` filtering in `s3_key`.
- **Acceptance:** Unit tests: endpoint `"minio.local:9000\u{7f}"` returns a localized error; profile `"../x"` fails validation; existing SigV4 test vectors still pass.

### SVC-16: `ProfileService::apply` unconditionally restores live files via `disable_takeover_for_app_sync` and flattens all errors into strings

**Type:** bug · **Priority:** P2 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/src/services/profile.rs:330-466` — for every app in scope it calls `state.proxy_service.disable_takeover_for_app_sync(app)` (361-367) before checking whether takeover is on; `src-tauri/src/services/proxy.rs:925-953` — that function always runs `restore_live_config_for_app_with_fallback_inner` (backup → SSOT write → placeholder cleanup), deletes the backup and clears provider health. All service errors are pushed as `format!` strings into `warnings` (profile.rs:380-443), including `SkillService` `anyhow` errors.
- **Problem:** Switching profiles rewrites every in-scope app's live file even when nothing changed and takeover is off (extra disk writes, mtime churn that triggers the session-log watchers of other tools), and the caller cannot tell a benign "skill already enabled" from a failed provider switch.
- **Proposal:** Check `get_proxy_config_for_app(..).enabled || detect_takeover_in_live_config_for_app` before restoring; return `Vec<ProfileWarning>` (typed enum with app/kind/message) instead of strings; make `apply` async once SVC-05 lands.
- **Acceptance:** Test that applying a profile with takeover off does not modify live file mtimes when the provider is already current; warnings serialize with a stable `code`.

### SVC-17: `check_updates` downloads every repository archive on each check, sequentially

**Type:** performance · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/services/skill.rs:871-983` — groups skills by `(owner,name,branch)` and for each group runs `download_repo` (full `archive/refs/heads/<branch>.zip`, up to three branch attempts at 2200-2236) with a 60 s timeout, then hashes directories; no ETag/commit SHA is stored.
- **Problem:** Users with 10 repos pay 10 full zip downloads per check (GitHub throttles unauthenticated downloads; 429 is surfaced only as a failure), and the checks run one after another.
- **Proposal:** Record the branch commit SHA (GitHub `GET /repos/{o}/{r}/commits/{branch}` or the `ETag` of the archive URL via HEAD) in `skill_repos`; only download when it changed; run groups with `futures::stream::buffer_unordered(3)`.
- **Acceptance:** With a mocked HTTP server, a second `check_updates` with unchanged SHA performs zero archive downloads; three repos complete in roughly one timeout window, not three.

### SVC-20: Collapse the four session-usage importers into a `SessionSource` trait plus one `UsageIngest` writer

**Type:** refactor · **Priority:** P2 · **Effort:** L · **Confidence:** high

- **Evidence:** `src-tauri/src/services/session_usage.rs:410-523` (`insert_session_log_entry`), `src-tauri/src/services/session_usage_codex.rs:595-710` (`insert_codex_session_entry`), `src-tauri/src/services/session_usage_gemini.rs:228-359` (`insert_gemini_session_entry`), `src-tauri/src/services/session_usage_opencode.rs:312-435` (`insert_opencode_message`) — the same timestamp parsing, `DedupKey`, `TokenUsage`, pricing lookup, cost formatting and 24-column INSERT, differing only in `provider_id`/`data_source` constants and INSERT-vs-UPSERT; `find_codex_pricing`/`find_gemini_pricing` (`codex.rs:710`, `gemini.rs:361`) are one-line wrappers of `find_model_pricing`; `get_codex_sync_state` (`codex.rs:171-221`) re-implements `session_usage::get_sync_state` with archive-cursor inheritance.
- **Problem:** Fixes land in one copy only (Claude uses `INSERT OR IGNORE`, Gemini an UPSERT with a change-detect WHERE, Codex/OpenCode their own variants); the grokbuild semantics bug (SVC-09) is the same class of drift. Adding Grok/Hermes session parsing means a fifth copy.
- **Proposal:** `trait SessionSource { fn app(&self)->&'static str; fn data_source(&self)->&'static str; fn discover(&self)->Vec<SourceFile>; fn parse(&self, file:&SourceFile, cursor:Cursor) -> Result<(Vec<UsageEvent>, Cursor)>; fn write_policy(&self)->WritePolicy /* Insert|Upsert */; }` and one `UsageIngest::run(db, &dyn SessionSource)` that owns dedup, pricing cache, cost, transaction and `update_sync_state`. Keep the per-source parsing tests; move insert tests to the ingest module.
- **Acceptance:** One INSERT statement text in the codebase for session rows; `sync_session_usage` iterates a `SESSION_SOURCES: &[&dyn SessionSource]` list; existing dedup/replay tests pass unchanged.

### SVC-21: Split `proxy.rs` (7.2k lines) into takeover submodules and convert its `Result<_, String>` API to `AppError`

**Type:** refactor · **Priority:** P2 · **Effort:** L · **Confidence:** high

- **Evidence:** `src-tauri/src/services/proxy.rs:1-3181` (non-test) and `3181-7215` (tests); 66 functions return `Result<_, String>`; responsibilities interleaved: Claude takeover field policy (82-340), start/stop (518-700), per-app takeover enable/disable (730-960), live→provider sync (960-1260), backup/restore/cleanup (1372-2140), crash recovery (2148-2200), hot switch (2284-2700), raw live file IO (2776-3010), config/circuit-breaker updates (3012-3180). Callers wrap with `AppError::Message(format!("...: {e}"))` (`provider/mod.rs:2319,2333,2342`), losing localization keys.
- **Problem:** Review and testing of takeover invariants (the most data-loss-prone area) is impractical in one file; stringly errors cannot be matched or localized by the frontend.
- **Proposal:** `services/proxy/{mod.rs (ProxyService facade), lifecycle.rs, takeover/{policy.rs, backup.rs, restore.rs, detect.rs}, hot_switch.rs, live_io.rs}`; introduce `#[derive(thiserror)] enum TakeoverError` converted into `AppError::localized` keys; keep tests next to the module they exercise.
- **Acceptance:** No file under `services/proxy/` exceeds ~1,200 lines; `rg "Result<.*, String>" src-tauri/src/services/proxy` is empty; frontend receives `takeover.*` localized keys.

### SVC-22: Three error idioms coexist (`anyhow`, `String`, `AppError`) and context is lost at every boundary

**Type:** tech-debt · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/services/skill.rs` uses `anyhow::Result` (30 references, e.g. `install` 567-575, `format_skill_error` JSON-in-string errors); `proxy.rs` (66), `balance.rs`, `coding_plan.rs`, `subscription.rs`, `env_manager.rs:20-40` return `Result<_, String>`; everything else uses `AppError`. `src-tauri/src/services/profile.rs:409-423` converts `anyhow` into warning strings; `src-tauri/src/commands/provider.rs:28-90` maps all to `.to_string()`.
- **Problem:** Localized errors (`AppError::Localized {key, zh, en}`) only survive on one of three paths; `anyhow` chains are flattened; the frontend cannot branch on error kind (e.g. retry on `usage_script.request_failed` is already special-cased by string key at `provider/usage.rs:60-70`).
- **Proposal:** Standardize on `AppError` with `#[from]` impls for `anyhow::Error`, `zip::result::ZipError`, `reqwest::Error`; add `AppError::Skill { code, params, hint }` to replace `format_skill_error`; let commands return `AppError` (it already implements `Serialize` for Tauri) instead of `String`.
- **Acceptance:** `rg "Result<.*, String>|anyhow::" src-tauri/src/services` is empty; a frontend-facing error carries `{key, message}` JSON for every service failure.

### SVC-23: No structured observability for the critical state machines

**Type:** tech-debt · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** 225 `log::` calls and zero `tracing`/metrics/`#[instrument]` across `src-tauri/src/services/`; messages mix Chinese and English; operation outcomes are strings (`SwitchResult { warnings: Vec<String> }` at `provider/mod.rs:112-117`, `backfill_failed:{id}` at 2657, `hermes_model_defaults_failed:{id}` at 2703); auto-sync failures are only `log::warn!` plus `settings.status.last_error` (`webdav_auto_sync.rs:92-96,163-165`); 29 `let _ =` in non-test `skill.rs` and the `let _` sites in `proxy.rs:1669-1705` discard errors without a log line.
- **Problem:** When a user reports "my key disappeared after switching", there is no per-operation trace that ties together backfill, strip, live write and MCP projection for one switch; support must reconstruct from unstructured logs.
- **Proposal:** Adopt `tracing` with a span per operation (`switch{app,from,to}`, `takeover{app}`, `sync{transport,direction}`, `skill_install{id}`) and events for each sub-step; emit an `operation-report` Tauri event with the structured outcome; replace `let _ =` with `log_err!(expr, "context")` helper or propagate; add a lightweight in-app "last 200 operations" ring buffer command for the support UI.
- **Acceptance:** A switch produces one span with child events visible in the log file; `rg "let _ = " src-tauri/src/services --glob '!**/tests*'` count drops to a handful of justified cases.

### SVC-26: Test fixtures are duplicated and env-mutating across three places

**Type:** test · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/services/provider/mod.rs:118-250` (`TempHome`, `test_guard`, `with_test_home` setting `HOME`/`USERPROFILE`/`CC_SWITCH_TEST_HOME`), `src-tauri/tests/support.rs` (85 lines, similar helpers), `src-tauri/tests/provider_service.rs:1-30` (`sanitize_provider_name`, `#[serial]` everywhere); provider builders (`codex_settings`, `openclaw_provider`, `hermes_provider`, `opencode_provider`) are redefined per file.
- **Problem:** Every test is serialized by a global mutex because paths come from process env; Windows-only path helpers are copy-pasted; new contributors add a fourth copy.
- **Proposal:** A `src-tauri/src/testkit.rs` (cfg(test)/feature `test-hooks`) exposing `TestEnv::new()` that returns an `AppState` whose path resolution is injected (thread the home dir through `AppState`/`AppAdapter` instead of reading env), plus provider/mcp/skill builders; delete the three copies.
- **Acceptance:** Tests run without `serial_test` for path isolation; one source of fixtures referenced by `tests/*.rs` and unit tests.

### SVC-27: Sync has no dry-run/diff: `fetch_remote_info` returns only manifest metadata

**Type:** feature · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/services/webdav_sync.rs:164-189` and `src-tauri/src/services/s3_sync.rs:134-165` — payload is device name, timestamps, snapshot id, artifact names and compatibility; `download` (`webdav_sync.rs:110-160`) goes straight to `apply_snapshot`.
- **Problem:** Users cannot see what a download will change (which providers/MCP servers/skills/settings differ, which device wrote it) before their local state is replaced; combined with SVC-02 this is how edits get lost without anyone noticing.
- **Proposal:** `sync_preview(transport) -> SyncDiff` that downloads `db.sql` into an in-memory SQLite (`Database::memory()` exists), runs table-level diffs for `providers`, `mcp_servers`, `skills`, `prompts`, `settings` (added/removed/changed ids with field-level diff for `settings_config` minus secrets) and compares `skills.zip` entry hashes against the SSOT; expose via a command and reuse the result as the conflict payload for SVC-02/SVC-28.
- **Acceptance:** Preview on identical snapshots returns an empty diff; changing one provider's base URL locally produces a one-entry `changed` list naming the key path; secrets are masked in the diff.

### SVC-28: Expose typed warnings and a "live ownership" query so the UI can render conflict/ownership state

**Type:** feature · **Priority:** P2 · **Effort:** M · **Confidence:** medium

- **Evidence:** `src-tauri/src/services/provider/mod.rs:112-117,2655-2660,2696-2705` — `SwitchResult.warnings` are ad-hoc `code:id` strings; takeover ownership is inferred ad hoc by three different signals (`provider/mod.rs:2298-2309`: backup row, placeholder scan, `is_running`); external edits to live files are only detected implicitly by the backfill on the next switch.
- **Problem:** The frontend cannot explain "why does my edit not show up in `~/.claude/settings.json`" (proxy owns it), "why did my MCP server reappear", or offer "keep live / keep DB" choices when live was edited outside cc-switch.
- **Proposal:** `enum SwitchWarning { BackfillFailed{provider}, CommonConfigSyncFailed{provider}, HermesDefaultsFailed{provider}, McpProjectionFailed{app, error}, BackfillSkippedOwnershipMismatch{..} }` serialized with `code` + params; `get_live_ownership(app) -> { owner: Provider(id) | ProxyTakeover | Unknown, live_hash, last_written_hash, externally_modified: bool }` backed by the `live_written_for` record from SVC-01; a `reconcile_live(app, Strategy::KeepLive|KeepDb)` command.
- **Acceptance:** Frontend i18n keys map 1:1 to warning codes; a test modifies the live file externally and asserts `externally_modified == true` and that `KeepDb` rewrites it while `KeepLive` backfills it.

### SVC-29: No scheduled database backups or retention; only the legacy `config.json` has a backup rotation

**Type:** feature · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/services/config.rs:10-85` — `create_backup`/`cleanup_old_backups` rotate 10 copies of the legacy JSON config only; `src-tauri/src/database/backup.rs:103` — `backup_database_file` runs only before an SQL import; no periodic snapshot of `cc-switch.db` exists in `services/` or `lib.rs` (the periodic task at `lib.rs:1095-1150` only syncs usage).
- **Problem:** The SQLite file is the SSOT for every secret and setting; a corrupt WAL, a bad migration, or a mistaken "download from cloud" has no local point-in-time recovery except the single pre-import copy.
- **Proposal:** `BackupService` with `snapshot_now()` (SQLite online backup API or `VACUUM INTO` to `~/.cc-switch/backups/db/<ts>.db`), a daily schedule plus pre-migration/pre-sync-download hooks, retention (N daily / M weekly), and `list/restore/delete` commands; skills SSOT optional zip alongside (reuse `zip_skills_ssot`).
- **Acceptance:** After startup a snapshot exists; restore from a snapshot round-trips providers; retention prunes beyond the configured counts; download/import paths create a snapshot first.

### SVC-30: Balance/coding-plan/subscription quota clients are 16 hand-rolled HTTP functions with no shared abstraction or mocked tests

**Type:** refactor · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/services/balance.rs:74-415` — five `query_*` functions each building a request with `.timeout(15s)`, parsing JSON and mapping auth errors; `src-tauri/src/services/coding_plan.rs:102-1270` — eight more (`query_kimi`, `query_zhipu`, `query_minimax`, `query_zenmux`, `query_volcengine` with its own SigV4-like signer at 798-980, `query_zhipu_team*`) dispatched by `detect_provider` substring matching on `base_url` (`balance.rs:26-43`, `coding_plan.rs:25-46`); `subscription.rs:1233-1347` repeats the credential-status state machine three times.
- **Problem:** Each new vendor adds ~80 lines of near-duplicate code; `detect_provider` keyword matching silently mis-routes custom gateways; none of the HTTP paths are tested (41 tests in `coding_plan.rs` cover only JSON tier parsing).
- **Proposal:** `trait QuotaProvider { fn matches(&self, base_url:&Url)->bool; async fn query(&self, http:&Client, creds:&Creds)->Result<QuotaResult>; }` with a registry; shared `json_get(url, headers)` helper with uniform auth-error mapping; `wiremock` tests per provider with recorded fixtures; Volcengine signing moved to a `sigv4` module shared with `s3.rs` (both implement HMAC-SHA256 canonical signing).
- **Acceptance:** Adding a provider is one file + one registry entry; each provider has at least one success and one 401 fixture test.

### SVC-31: `ConfigService::sync_current_providers_to_live` is legacy dead code kept alive only by tests

**Type:** tech-debt · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/services/config.rs:88-266` — operates on the pre-DB `MultiAppConfig` and re-implements Codex/Claude/Gemini live write + read-back-backfill; the only callers are `src-tauri/tests/import_export_sync.rs:47,104,186,261`; no production call site (`rg "ConfigService::" src-tauri/src` finds only the re-export in `lib.rs:63`).
- **Problem:** Two implementations of "write live and backfill" can drift (this one has no common-config handling, no takeover awareness, no MCP projection), and the tests pass against behavior users never exercise.
- **Proposal:** Delete the function trio (`sync_current_providers_to_live`, `sync_codex_live`, `sync_claude_live`, `sync_gemini_live`), keep `create_backup` (used for legacy config), and re-point the four tests at `ProviderService::switch`/`sync_current_provider_for_app`.
- **Acceptance:** `config.rs` contains only backup helpers; `import_export_sync.rs` tests exercise `ProviderService`.

### SVC-41: The live/backfill/takeover ownership model is documented only in scattered Chinese inline comments

**Type:** docs · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** The rules that make the layer safe live in long comments: `src-tauri/src/services/provider/mod.rs:2878-2915` (common-config whole-replace rationale), `src-tauri/src/services/provider/live.rs:762-905` (what is stripped on backfill and why), `src-tauri/src/services/proxy.rs:1372-1384,1857-1930` (why backups skip placeholder live files, three-layer restore fallback), `src-tauri/src/services/provider/mod.rs:2294-2305` (ownership signals); there is no `services/README.md` or architecture doc; log and error strings alternate between Chinese and English (e.g. `provider/mod.rs:2319` vs `skill.rs:1668`).
- **Problem:** Reviewers and contributors cannot verify invariants without reading thousands of lines; the same invariant (who owns live) is restated slightly differently in four places, which is how SVC-06 happened.
- **Proposal:** Add `src-tauri/src/services/README.md` with: the SSOT ↔ live ↔ proxy-backup state machine (one diagram), the ownership signals and their precedence, the backfill strip list per app, the sync protocol layout and compatibility rules, and error/logging conventions (English-first messages, localized keys for user-facing errors). Link each section from the relevant module docs.
- **Acceptance:** README exists and is referenced from `CLAUDE.md`/CONTRIBUTING; new app adapters (SVC-18) must fill a checklist from it.

### SVC-32: Deprecated v3.6 compatibility APIs and dead code are still exported

**Type:** tech-debt · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/services/mcp.rs:248-292` — `#[deprecated] get_servers/set_enabled/sync_enabled` ("will be removed in v4.0"); `src-tauri/src/services/skill.rs:1643-1646` — `#[deprecated] copy_to_app`; `src-tauri/src/services/mod.rs:37-46` — `#[allow(unused_imports)]` re-exports; `src-tauri/src/services/provider/live.rs:945-1014` — `#[allow(dead_code)] LiveSnapshot` with an unused `restore`.
- **Problem:** Dead/deprecated surface misleads readers about which path is canonical and hides a useful primitive (`LiveSnapshot::restore` is the rollback SVC-01 needs).
- **Proposal:** Remove the deprecated functions (grep frontend for the old command names first), drop the `allow`s, and either use `LiveSnapshot` in the switch rollback or delete it.
- **Acceptance:** No `#[deprecated]`/`#[allow(dead_code)]`/`#[allow(unused_imports)]` in `services/`; `cargo clippy -D warnings` passes.

### SVC-33: `McpService::import_from_*` is six copies of the same loop

**Type:** tech-debt · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/services/mcp.rs:294-513` — `import_from_claude/codex/gemini/grokbuild/opencode/hermes` differ only in the `crate::mcp::import_from_*` call and the `merged.apps.<app> = true` line; `import_from_all_apps` (515-546) enumerates them by hand.
- **Problem:** Adding OpenClaw MCP import (noted as in progress at `mcp.rs:139-141`) means a seventh copy; per-app merge semantics can diverge.
- **Proposal:** `fn import_from_app(state, app: &AppType) -> Result<usize>` using `AppMcpApps::set_enabled_for(app, true)` and a `match` (or the `AppAdapter::mcp_import`) for the source call; `import_from_all_apps` iterates `AppType::all()`.
- **Acceptance:** One import loop; `tests/mcp_commands.rs` import tests pass unchanged.

### SVC-34: Additive-app live import loops are triplicated and there is no importer for other config managers

**Type:** feature · **Priority:** P3 · **Effort:** M · **Confidence:** medium

- **Evidence:** `src-tauri/src/services/provider/live.rs:1688-1760,1763-1848,1851-1916` — `import_opencode/openclaw/hermes_providers_from_live` share the same "existing → compare → update, else create with `live_config_managed: true`" loop; `import_default_config` (1427-1570) handles the exclusive apps; nothing imports from other provider managers or shells (e.g. env-exported `ANTHROPIC_*`, Claude Code Router `config.json`, Continue/Cline settings).
- **Problem:** Users migrating from env-var based setups or other switchers re-enter every key by hand; the triplicated loop will become a fourth copy for the next additive app.
- **Proposal:** `trait ProviderImporter { fn id(&self)->&str; fn detect(&self)->bool; fn read(&self)->Result<Vec<ImportedProvider>>; }` with impls for each live file and for env vars/shell rc (reuse `env_checker::check_shell_configs`) and 1-2 popular third-party formats; one `import_providers(state, importer, app)` loop with dedup-by-base-url+key.
- **Acceptance:** Import from a shell rc containing `ANTHROPIC_BASE_URL`/`ANTHROPIC_AUTH_TOKEN` creates a Claude provider; the three additive import functions reduce to one loop.

### SVC-35: Env-var profiles: the env tooling only detects/deletes, it cannot export a provider as a shell profile

**Type:** feature · **Priority:** P3 · **Effort:** M · **Confidence:** medium

- **Evidence:** `src-tauri/src/services/env_checker.rs:35-42` — keyword lists for only claude/codex/gemini; `src-tauri/src/services/env_manager.rs:20-40` — delete/restore only; `Provider::resolve_usage_credentials` (used at `provider/usage.rs:108`) already knows how to derive `(base_url, api_key)` per app.
- **Problem:** Users who run CLIs in CI, containers, or shells without the live files cannot reuse their cc-switch providers; the UI also cannot tell them which env vars currently shadow the live config for Grok/OpenCode/Hermes.
- **Proposal:** `EnvProfileService::render(app, provider_id, Shell::{Bash,Fish,PowerShell,Dotenv}) -> String` producing properly quoted `export` lines (and a `direnv`/`.env` variant) from the adapter's credential mapping; extend `get_keywords_for_app` to all apps (`XAI_`, `OPENCODE_`, `HERMES_`), and add "copy as env" to the provider menu.
- **Acceptance:** Rendered Bash profile for a Claude provider sets `ANTHROPIC_BASE_URL`/`ANTHROPIC_AUTH_TOKEN` with shell quoting verified by a round-trip through `shlex`; env conflict check covers all `AppType::all()`.

### SVC-36: Sync materializes whole artifacts (up to 512 MB each) in memory

**Type:** performance · **Priority:** P3 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/services/sync_protocol.rs:36,105-123` — `MAX_SYNC_ARTIFACT_BYTES = 512 MB`; `build_local_snapshot` holds `db_sql` and `skills_zip` as `Vec<u8>` plus the manifest; `src-tauri/src/services/webdav.rs:253-297` and `src-tauri/src/services/s3.rs:420-470` — `get_bytes`/`get_object` accumulate the body into a `Vec` up to `max_bytes`; `archive.rs:94-100` then writes the zip back to a temp file to extract it.
- **Problem:** A skills SSOT of a few hundred MB (skills with vendored binaries are common) creates a transient 2–3× memory spike in a desktop app and a long single-threaded SHA pass.
- **Proposal:** Stream uploads from temp files (`reqwest::Body::wrap_stream`), stream downloads to temp files with an incremental SHA-256, and compute artifact hashes while zipping; keep the size caps as streaming limits.
- **Acceptance:** Peak RSS during a 300 MB skills sync stays within ~50 MB of baseline (manual measurement); hash verification still rejects tampered artifacts.

### SVC-37: `sync_all_providers_to_live` reports success when every provider failed to write

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/services/provider/live.rs:1173-1200` — per-provider failures are `log::warn!` + `continue` and the function returns `Ok(())`; it is used by `sync_current_to_live` (1278-1306) during config import and cloud restore, and it rewrites the live file once per provider rather than once.
- **Problem:** After a cloud download the UI reports success while OpenCode/OpenClaw/Hermes live files may contain none of the restored providers; N providers mean N read-modify-write cycles of `opencode.json`.
- **Proposal:** Build the full additive provider map and write it once per app (the `*_config::write_*` modules already merge maps), and return an aggregated error/warning list like `McpService::sync_all_enabled` does.
- **Acceptance:** A test with one invalid OpenCode provider among three asserts the other two are present in live and the call returns a warning naming the failed id.

### SVC-38: `SkillService` discards errors in uninstall/migration paths with `let _ =`

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/services/skill.rs:788-790` — `uninstall` ignores every `remove_from_app` error; `skill.rs:1191-1210` — `migrate_storage` ignores `remove_dir_all(&src)` after a copy fallback and ignores `sync_to_app` failures for all apps; 29 `let _ =` sites in non-test `skill.rs` overall.
- **Problem:** After an uninstall, a stale symlink or copied directory can remain in `~/.claude/skills` with no DB record (the skill "comes back" as unmanaged); after migration the old SSOT may still hold copies and app dirs may point at the old location, with `MigrationResult.errors` empty.
- **Proposal:** Collect failures into `SkillUninstallResult.warnings` / `MigrationResult.errors`, log at warn with the path, and let the UI show them.
- **Acceptance:** Tests with a read-only app skills dir assert `uninstall` returns a warning naming the path and `migrate_storage.errors` is non-empty.

### SVC-39: Prompt service writes DB and file in opposite orders and silently backfills DB content from the live file

**Type:** bug · **Priority:** P3 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/src/services/prompt.rs:28-56` — `upsert_prompt` saves the DB row then writes the live file (file failure leaves an enabled prompt whose content is not live); `prompt.rs:73-143` — `enable_prompt` overwrites the currently enabled prompt's DB `content` with whatever is in the live file (82-92) and otherwise creates `backup-<ts>` prompts (96-118), then writes the file before the DB (132-133) and saves every prompt afterwards (139-141).
- **Problem:** Editing a prompt in the UI and immediately enabling another can lose the UI edit if the live file was stale; repeated enable toggles accumulate backup prompts; partial failure can leave two enabled prompts.
- **Proposal:** Treat the live prompt file like provider live files: track a `live_written_hash`, only backfill when the file hash differs from what cc-switch last wrote, and do the DB updates in one transaction (SVC-11) before the file write.
- **Acceptance:** Enabling prompt B after editing prompt A in the UI keeps A's edited content; toggling A/B 10 times creates at most one backup prompt.

### SVC-40: Subscription quota refresh re-fetches a new Gemini access token on every call and embeds the CLI client secret

**Type:** tech-debt · **Priority:** P3 · **Effort:** S · **Confidence:** medium

- **Evidence:** `src-tauri/src/services/subscription.rs:965-998` — `GEMINI_OAUTH_CLIENT_SECRET` constant (documented as the public Gemini CLI value) and `refresh_gemini_token` whose result is not persisted or cached; `subscription.rs:1304-1330` — once the stored token is `Expired`, every quota poll performs a refresh round-trip; the tray auto-refresh (`usage_cache.rs` docs) calls this periodically.
- **Problem:** Unnecessary token-endpoint traffic (Google rate limits refreshes), and the secret's provenance is only a comment, which will raise questions in security review.
- **Proposal:** Cache the refreshed `(access_token, expires_at)` in `UsageCache`/memory and reuse until expiry; move the client id/secret behind a documented `gemini_oauth` module with a link to the upstream source and a build-time override.
- **Acceptance:** Two consecutive Gemini quota queries within an hour perform one refresh (mock server counts).
