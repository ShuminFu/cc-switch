# Config adapters: ownership, backfill and backups

This page is for contributors. It describes which parts of each tool's live
configuration cc-switch owns, what happens on a switch, and where to look when
a user reports a lost or changed file. The code lives in
`src-tauri/src/adapters/` (one module per app, registered in
`adapters::adapter_for`) plus the per-app modules at the crate root
(`codex_config.rs`, `gemini_config.rs`, `hermes_config.rs`, ...).

## Single source of truth

The database (`~/.cc-switch/cc-switch.db`) is the source of truth for
providers and MCP servers. Live files are projections of it:

- **Switch** (exclusive apps): backfill the live snapshot into the _current_
  provider (`AppAdapter::backfill` strips what cc-switch injected), then write
  the target provider (`AppAdapter::write_live`).
- **Additive apps** (OpenCode, OpenClaw, Hermes): every provider coexists in
  one file. A write only touches that provider's entry.
- **MCP**: `McpProjector::project` writes enabled servers and removes disabled
  ones. An existing live entry is merged, not replaced. Transport keys come
  from the DB; other keys the app added are kept (`mcp::merge`).
- **Proxy takeover**: the live file is backed up, then replaced by a
  placeholder that points at the local proxy. Disabling takeover restores the
  backup.

## Rules every writer follows

| Rule                                                                                  | Where                                                             |
| ------------------------------------------------------------------------------------- | ----------------------------------------------------------------- |
| An unparsable live file is never overwritten; the operation fails                     | each adapter's reader                                             |
| Writes are atomic: tmp file, fsync, rename. A symlink target is updated in place      | `config::atomic_write*`                                           |
| A file whose content would not change is not rewritten                                | `live_file::write_live_text`                                      |
| The previous content is backed up before a replace                                    | `config::backup_live_file` → `~/.cc-switch/backups/<app>/`        |
| JSON key order, indent, BOM and EOL are preserved. JSONC/JSON5 comments are preserved | `live_file::{to_json_string, write_json_preserving}`, `json5_doc` |
| New files that carry credentials are created `0600`                                   | `WriteOptions::secret`, `*_secret` helpers                        |
| Secrets-vault placeholders (`cc-switch-secret:v1:`) are never written to a live file  | `live_file::ensure_no_vault_placeholder`                          |

## Directory resolution

The same order applies to every app: the cc-switch setting (Settings →
directories), then the tool's own environment variable, then the platform
default. Helpers: `config::resolve_tool_dir`, `resolve_*_dir()` (which also
return `ConfigDirSource`). Tool environment variables are ignored when
`CC_SWITCH_TEST_HOME` is set.

## Ownership matrix

| App            | Files (format)                                                                                              | Mode      | cc-switch owns                                                                                                                                             | Stripped on backfill                                                                                                             | Env override                                    | Backups                                                                                |
| -------------- | ----------------------------------------------------------------------------------------------------------- | --------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------- | -------------------------------------------------------------------------------------- |
| Claude Code    | `~/.claude/settings.json` (JSON); `~/.claude.json` (JSON, owned by Claude Code)                             | exclusive | provider keys in `settings.json` (`env`, model keys). In `~/.claude.json`, only `mcpServers.<id>` and `hasCompletedOnboarding`                             | MCP projections, cc-switch defaults                                                                                              | `CLAUDE_CONFIG_DIR` (also moves `.claude.json`) | `backups/claude/`                                                                      |
| Codex          | `~/.codex/auth.json` (JSON), `config.toml` (TOML), `cc-switch-model-catalog.json`                           | exclusive | `config.toml` routing (`model_provider`, `model_providers.*`, model keys), `[mcp_servers.<id>]`. `auth.json` only for official providers or in compat mode | `[mcp_servers]`, the unified-session `custom` route, the live `experimental_bearer_token` (lifted back to `auth.OPENAI_API_KEY`) | `CODEX_HOME`                                    | `backups/codex/`; the two-file write journal is `~/.cc-switch/journal/codex-live.json` |
| Gemini CLI     | `~/.gemini/.env` (dotenv), `settings.json` (JSONC)                                                          | exclusive | `.env` keys; `security.auth.selectedType`; `mcpServers.<id>`                                                                                               | MCP projections                                                                                                                  | `GEMINI_CLI_HOME` (`$GEMINI_CLI_HOME/.gemini`)  | `backups/gemini/`                                                                      |
| Grok Build     | `~/.grok/config.toml` (TOML)                                                                                | exclusive | provider tables, `[mcp_servers.<id>]`                                                                                                                      | MCP projections                                                                                                                  | none                                            | none                                                                                   |
| OpenCode       | `~/.config/opencode/opencode.json` (JSONC)                                                                  | additive  | `provider.<id>`, `mcp.<id>`                                                                                                                                | none                                                                                                                             | `XDG_CONFIG_HOME`                               | `backups/opencode/`                                                                    |
| OpenClaw       | `~/.openclaw/openclaw.json` (JSON5)                                                                         | additive  | `models.providers.<id>`, default model                                                                                                                     | none                                                                                                                             | none                                            | `backups/openclaw/`                                                                    |
| Hermes         | `~/.hermes/config.yaml` (YAML)                                                                              | additive  | `custom_providers[name=<id>]`, `model`, `mcp_servers.<id>` (a `null` field in a provider update deletes that key)                                          | none                                                                                                                             | `HERMES_HOME`                                   | `backups/hermes/`                                                                      |
| Claude Desktop | `claude_desktop_config.json` (normal and `-3p`), `configLibrary/<profile>.json`, `configLibrary/_meta.json` | exclusive | `deploymentMode`, the cc-switch profile and its `_meta.json` entry                                                                                         | none                                                                                                                             | none                                            | rollback snapshot during apply                                                         |

Claude Desktop profiles allow Cowork egress only to the gateway host (direct)
or the proxy origin (proxy). Extra hosts, including `"*"`, need an explicit
`settings_config.coworkEgressAllowedHosts`.

## Codex specifics

- With the default setting `preserve_codex_official_auth_on_switch = true`,
  a third-party switch writes only `config.toml`, with a provider-scoped
  `experimental_bearer_token`, and the ChatGPT login in `auth.json` is kept.
  In compat mode (`false`), `auth.json` is replaced, and a login cache is
  backed up first.
- Switching to an official provider whose stored auth has no login removes a
  leftover third-party `OPENAI_API_KEY` from `auth.json`.
- The model-catalog template never spawns the `codex` CLI on the switch path.
  The order is `models_cache.json`, then
  `~/.cc-switch/cache/codex-model-template.json` (filled by a background
  probe with a 5 s timeout), then the bundled template.

## Startup migrations and their backups

| Migration                                                          | Backups                                                          | Retention                      |
| ------------------------------------------------------------------ | ---------------------------------------------------------------- | ------------------------------ |
| Third-party history bucket (`codex-history-provider-migration-v1`) | `~/.cc-switch/backups/codex-history-provider-migration-v1/<ts>/` | newest N (backup retain count) |
| Official history unify (`codex-official-history-unify-v1`)         | `~/.cc-switch/backups/codex-official-history-unify-v1/<ts>/`     | kept: it is the restore ledger |

Session files modified in the last 15 minutes may still be open in Codex.
They are deferred, and the completion marker is not written until a later
startup has migrated them. A file that cannot be read (for example, invalid
UTF-8) is logged and skipped, and the other files are still migrated.

## Where are my backups?

Everything lives under `~/.cc-switch/backups/`: one folder per app
(`claude`, `codex`, `gemini`, `opencode`, `openclaw`, `hermes`), plus the
migration folders above. Files are named `<original-stem>_<timestamp>.<ext>`.

The migrations are registered in `src-tauri/src/migrations.rs` and run by
`migrations::run_pending` on a background thread at startup. Migrations
that rewrite Codex session files are deferred while a Codex state DB
`-wal` file changed in the last two minutes (Codex is probably running).
`get_codex_history_backup_usage` reports the backup size, and
`purge_codex_history_backups(includeRestoreLedger)` deletes the backups.
The restore ledger is kept unless `includeRestoreLedger` is true.

## Diagnostics and previews (commands)

| Command                                     | Returns                                                                                                                                                            |
| ------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `get_app_config_dirs`                       | `{ app, path, exists, source }[]`. `source` is `{ kind: "override" }`, `{ kind: "env", var: "CODEX_HOME" }` or `{ kind: "default" }`.                              |
| `get_mcp_sync_skips`                        | `{ app, missingDir }[]` for the MCP apps whose config directory is missing. `upsert_mcp_server` and `toggle_mcp_app` return the same list for the apps they wrote. |
| `get_import_candidates(app)`                | `{ idHint, name, settingsConfig, fingerprint }[]`. These contain credentials.                                                                                      |
| `preview_live_write(app, providerId)`       | `{ app, providerId, supported, files: [{ path, before, after, changed, unifiedDiff }] }`                                                                           |
| `validate_provider_settings(app, settings)` | `{ path, key, params }[]`: advisory field issues, such as `model_providers.custom.wire_api` with the key `validation.codex.wireApi.invalid`.                       |

`preview_live_write` applies the common config the same way a switch does,
and then asks the adapter's `plan_live` for the exact bytes. An adapter that
implements `plan_live` writes those same bytes in `write_live`, so the
preview is what the switch writes. Only Claude Code has a planner today.
The other apps return `supported: false`. MCP projection and side files
(Codex model catalog, OMO) are not part of the plan yet.
