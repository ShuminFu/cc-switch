# `services/` — invariants and conventions

This is the reference for the rules that keep the SQLite database, each app's
"live" config files and the proxy-takeover backup consistent. Module docs link
here instead of restating them. When you add an app adapter
(`src-tauri/src/adapters/`), go through the checklist at the end.

## 1. Three copies of a provider

| Copy | Where | Owner |
| --- | --- | --- |
| **SSOT** | `providers` table (`~/.cc-switch/cc-switch.db`) | cc-switch; the only copy that is synced to the cloud |
| **Live** | the app's own files (`~/.claude/settings.json`, `~/.codex/config.toml` + `auth.json`, `~/.gemini/.env` + `settings.json`, `~/.grok/…`, `opencode.json`, …) | the app; cc-switch writes it on switch/save |
| **Takeover backup** | `proxy_live_backup` table, one row per app | the proxy; holds the *real* live config while the live file points at the local proxy |

### State machine (switch-mode apps: Claude, Codex, Gemini, Grok Build, Claude Desktop)

```
             switch / save (live owner = provider)
  ┌────────────────────────────────────────────────────────┐
  │                                                        ▼
[DIRECT] ──set_takeover_for_app(app, true)──▶ [TAKEN OVER] ──set_takeover_for_app(app, false)──▶ [DIRECT]
  live = provider config                      live = proxy placeholders            backup restored to live,
  no backup row                               backup row = real config             backup row deleted
                                              switch/save update the BACKUP,
                                              never the live file
```

Additive-mode apps (OpenCode, OpenClaw, Hermes) have no "current" provider and
no takeover: every managed provider (`meta.live_config_managed != false`) is
written into the one live file.

## 2. Who owns the live file (ownership signals)

Before writing a live file, decide who owns it. The signals, strongest first:

1. **A takeover backup row exists** for the app (`db.get_live_backup(app)`).
2. **The live file carries proxy placeholders** (`ProxyService::detect_takeover_in_live_config_for_app`):
   `PROXY_MANAGED` tokens / a `127.0.0.1:<port>` base URL.
3. **`proxy_config.enabled`** for the app (the per-app flag).

If (1) or (2) holds, the proxy owns the live file: provider edits go to the
backup via `ProxyService::update_live_backup_from_provider`, and the live file
is left alone. Do not rely on (3) alone — a crash can leave the flag and the
files disagreeing. `ProxyService::takeover_engaged_for_app_sync` combines all
three for callers that only need a yes/no (e.g. profile apply skips the
restore when it is false).

Takeover is per app only. There is no global takeover entry point any more
(`start_with_takeover` was removed, SVC-51); `recover_from_crash` is the only
global path and restores every app that still has a backup.

## 3. Backfill (live → SSOT)

When switching away from provider A, the live file may contain edits the user
made outside cc-switch. They are copied back into A's SSOT row ("backfill")
after removing everything cc-switch itself injected:

| App | Stripped before backfill |
| --- | --- |
| Claude | the common-config snippet (`strip_common_config_from_live_settings`), injected Codex-OAuth / Kimi-for-coding context defaults (`strip_injected_claude_defaults`) |
| Codex | the common-config TOML, the generated `experimental_bearer_token` (the key lives in `auth.OPENAI_API_KEY`) |
| Gemini | the common-config snippet; the `security.auth.selectedType` flag is derived, not stored |
| all | proxy placeholders — never backfill a taken-over live file |

The common-config snippet is applied as a whole-replace on write and stripped
as a subset on read; keep the two symmetric (see `provider/live.rs`).

Prompts follow the same idea (SVC-39): the live prompt file is only backfilled
into the enabled prompt when its hash differs from what cc-switch last wrote
(`prompt_live_written_hash_<app>` setting).

## 4. Write ordering

* Validate first, then write live, then persist the DB — or persist the DB in
  a way that can be rolled back if the live write fails. A failed live write
  must not leave a DB pointer to a provider whose config is not live
  (SVC-01).
* Multi-file projections (MCP to up to six apps) are applied app by app and
  reverted for the apps already touched if a later one fails (SVC-12).
* Best-effort loops (sync all providers, sync all MCP apps) must still report
  what failed — aggregate the ids/apps into the returned error instead of
  logging and returning `Ok` (SVC-37).

## 5. Cloud sync protocol

* Layout: `<remote_root>/v<PROTOCOL_VERSION>/db-v<DB_COMPAT_VERSION>/<profile>/`
  holding `manifest.json`, `db.sql`, `skills.zip`. `remote_root` and `profile`
  may not contain `.`/`..` segments or control characters
  (`sync_protocol::validate_remote_layout`). WebDAV also reads the legacy
  layout.
* Upload: artifacts first, manifest last, manifest `PUT` with `If-Match`.
  Refused (conflict) when the remote manifest changed since the last sync,
  unless forced.
* Download: refused when local content changed since the last sync, unless
  forced; artifacts are hash-verified against the manifest, then
  `apply_snapshot` replaces the skills SSOT (two renames, recovered on start
  if interrupted — SVC-53) and imports `db.sql` through the authorizer-guarded
  importer.
* Auto-sync: one worker for every transport (`auto_sync.rs`). A download holds
  an `AutoSyncSuppressionGuard`; batches queued before it, and uploads whose
  local fingerprint is unchanged, are skipped (SVC-52).

## 6. Errors and logs

* User-facing errors: `AppError::localized(key, zh, en)`; the key is what the
  frontend translates. Pick a dotted key under the module's namespace
  (`provider.*`, `sync.*`, `proxy.*`, `skill.*`).
* Log messages: English for new code; include the app and id/path.
* Never `let _ =` an error that changes what the user sees. Either propagate
  it, aggregate it into a warning list, or log it at `warn` with context.

## 7. New app adapter checklist

- [ ] Live file paths and read/write in the adapter, writes atomic.
- [ ] Mode: switch or additive (`AppType::is_additive_mode`).
- [ ] Takeover: placeholder fields, detection, backup and restore.
- [ ] Backfill strip list (section 3) and a round-trip test.
- [ ] MCP projector (or explicitly none) and skills dir.
- [ ] Env-conflict keywords (`env_checker::get_keywords_for_app`) and the
      env-profile mapping (`env_profile::env_vars_for`).
- [ ] Adapter conformance fixtures (`tests/adapter_conformance.rs`).
