# Health checks and "Copy diagnostics" (DOCS-15)

Status: **specification**. The backend command and the Settings UI are not implemented yet; this
page is the contract both sides build against. It also defines the text users paste into the
bug-report form (`.github/ISSUE_TEMPLATE/bug_report.yml`).

## Why

Health signals exist but are spread over five places: tool install/version/conflict diagnostics
in Settings → About (`get_tool_versions`, `probe_tool_installations`), environment-variable
conflicts (`check_env_conflicts`), proxy and takeover state (`get_proxy_status`,
`get_proxy_takeover_status`), the failover queue (`get_failover_queue`) and database backups
(`list_db_backups`). Bug reports retype version and OS by hand and rarely include the log lines
that matter.

## Command: `collect_diagnostics`

```rust
#[tauri::command]
#[specta::specta]
pub async fn collect_diagnostics(
    state: State<'_, AppState>,
    options: DiagnosticsOptions,
) -> Result<DiagnosticsReport, AppError>;

#[derive(Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsOptions {
    /// Number of trailing log lines to include (default 50, max 500).
    pub log_lines: Option<u32>,
}

#[derive(Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsReport {
    pub generated_at: String,        // RFC 3339, local offset
    pub app: AppInfo,                // version, build (release/debug), portable, install kind
    pub os: OsInfo,                  // family, version, arch, desktop/session on Linux
    pub language: String,            // UI language setting
    pub checks: Vec<HealthCheck>,
    pub log_tail: Vec<String>,       // already redacted
}

#[derive(Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheck {
    pub id: String,                  // stable, see the table below
    pub status: HealthStatus,        // "ok" | "warn" | "error" | "skipped"
    pub summary: String,             // English, one line, already redacted
    pub detail: Option<String>,      // paths, versions; already redacted
    pub fix: Option<HealthFix>,      // what the UI offers as a button
}

#[derive(Serialize, Type)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum HealthFix {
    OpenSettings { tab: String },    // e.g. "advanced", "proxy"
    OpenFolder { path: String },
    OpenDocs { topic: String },      // a DOCS-16 help topic
}
```

Rules:

- Read-only. It never writes a file, starts the proxy or calls the network, except the update
  check, which reuses the cached result of the last check (no new request; `PRIVACY.md` stays
  unchanged).
- Each check runs with a timeout (2 s); a check that times out reports `skipped` with the reason.
  One failing check never fails the command.
- `summary` and `detail` are English. The UI localises by `id` and `status`
  (`diagnostics.<id>.<status>`), and shows `detail` verbatim.
- The command lives in `src-tauri/src/commands/diagnostics.rs` and is typed
  (`pnpm gen:bindings`), with a default MSW handler in `tests/msw/handlers.ts` (AGENTS.md rule 5).

## Checks

| `id` | `ok` when | `warn` / `error` | Source today | Fix offered |
| --- | --- | --- | --- | --- |
| `database` | `~/.cc-switch/cc-switch.db` opens, schema at the latest migration | error: open/migrate failed (`get_init_error`) | `database/`, `get_init_error` | OpenFolder (config dir) |
| `config_dirs` | each enabled app's config dir exists and is writable | warn: missing; error: not writable | `get_config_dir`, `config.rs` | OpenSettings `advanced` |
| `cli_tools` | each enabled app's CLI is found and runs `--version` | warn: not installed or several installs on PATH | `get_tool_versions`, `probe_tool_installations` | OpenDocs `install-tools` |
| `env_overrides` | no environment variable overrides a managed key | warn: N variables (names only) | `check_env_conflicts` | OpenSettings `general` |
| `proxy` | proxy stopped, or listening on its configured port | error: configured port in use by another process | `get_proxy_status` | OpenSettings `proxy` |
| `takeover` | takeover state matches the live files (`is_live_takeover_active`) | warn: takeover flag on but live file not pointing at the proxy, or the reverse (residue) | `get_proxy_takeover_status`, `is_live_takeover_active` | OpenSettings `proxy` |
| `failover` | failover off, or on with a non-empty queue | warn: on with an empty queue | `get_failover_queue`, `commands/failover.rs` | OpenSettings `proxy` |
| `backups` | a database backup newer than 7 days exists | warn: none, or older than 7 days | `list_db_backups` | OpenSettings `advanced` |
| `update` | the last update check found no newer version | warn: a newer version exists | cached `check_for_updates` result | OpenSettings `about` |
| `crash_log` | no `crash.log` newer than the last start | warn: crash log present (path) | `lib.rs` panic hook | OpenFolder (config dir) |

A clean install with every app's CLI present reports every check `ok`.

## Redaction (AGENTS.md rule 7)

Applied in Rust before anything leaves the command, to `summary`, `detail` and every log line:

- API keys and tokens: values of keys matching `(?i)(api[_-]?key|token|secret|password|authorization)`
  and strings matching common key shapes (`sk-…`, `sk-ant-…`, `AKIA…`, long base64/hex runs)
  become `[redacted]`.
- URLs keep scheme and host; query strings become `?[redacted]` (deep links included).
- The home directory prefix becomes `~`.
- Environment-variable conflicts list names only, never values.

Tests: a unit test feeds a log with each secret shape and asserts none survives; a contract test
asserts the report JSON never contains the fixture keys used elsewhere in the suite.

## "Copy diagnostics" text

The UI renders the report as Markdown whose first lines map one-to-one onto the bug-report form
fields, so a pasted report fills them:

```text
CC Switch Version: 3.17.0 (release, installer: Windows MSI)
Operating System: Windows
OS Version: Windows 11 24H2 (26100)
Architecture: x64 / Intel
UI language: en

Checks:
- [ok] database: cc-switch.db schema v22
- [warn] env_overrides: 1 variable overrides managed keys (ANTHROPIC_BASE_URL)
- [ok] proxy: stopped
...

Last 50 log lines (redacted):
...
```

The architecture line uses the exact option text of the form's dropdown
(`x64 / Intel`, `ARM64 (Windows on ARM, Linux ARM64)`, `Apple Silicon (M1/M2/M3/M4)`).
Once the button ships, `bug_report.yml` gains an optional "Diagnostics" textarea that asks for
this text.

## UI (not part of this change)

A "Health" section in Settings → About lists the checks with their status and fix buttons, plus a
"Copy diagnostics" button that copies the text above. The component lives in
`src/components/settings/` and calls the command through `src/lib/api`.
