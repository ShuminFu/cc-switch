# `ccs` command-line companion and headless proxy

`ccs` is a small command-line tool that works on the same SQLite database and
live CLI config files as the CC Switch desktop app. It links the same Rust
library and calls the same services as the GUI (`ProviderService::switch`,
`ProxyService`, `Database::export_sql` / `import_sql`), so a switch from the
terminal writes `~/.claude/settings.json`, `~/.codex/config.toml`, takeover
backups and so on exactly like a click in the app.

The desktop binary also has a `--headless` mode that runs the local proxy
without creating a window or tray, for servers, WSL and SSH sessions.

## Building

```bash
cd src-tauri
cargo build --release --bin ccs        # target/release/ccs
cargo build --release --bin cc-switch  # desktop app (also provides --headless)
```

`ccs` is a separate `[[bin]]` in `src-tauri/Cargo.toml`; `default-run` stays
`cc-switch`, so `cargo run` and `tauri dev/build` keep launching the desktop
app. Installers do not ship `ccs` yet; copy the binary onto your `PATH`.

## Commands

| Command | What it does | Takes the writer lock |
| --- | --- | --- |
| `ccs list [app]` (alias `ls`) | List providers; `*` marks the current one | no |
| `ccs current [app]` | Show the current provider (all apps, or one) | no |
| `ccs switch <app> <provider>` | Switch an app to a provider, by id or exact name (case-insensitive) | yes |
| `ccs proxy status` | Is the proxy running, who runs it, which apps are taken over | no |
| `ccs proxy start [--no-takeover]` | Run the proxy in the foreground, headless (Ctrl-C to stop) | yes, while running |
| `ccs proxy stop [--timeout N]` | Stop a headless proxy started by `ccs proxy start` / `cc-switch --headless` | no |
| `ccs usage summary [--days N] [--app APP]` | Request/token/cost totals for the last N days (default 30) | no |
| `ccs profile list` (alias `ls`) | List projects (profiles) and which one is current per app | no |
| `ccs profile apply <app> <project>` | Apply a project's saved configuration to one app, by id or exact name (same as the project switcher) | yes |
| `ccs doctor` | Read-only checks: data directory writable, database present and supported, writer lock, takeover residue, dangling current-provider pointers | no |
| `ccs export <file>` | Write a SQL backup (same as Settings → Export) | no |
| `ccs import <file>` | Import a SQL backup and re-sync live files (same as Settings → Import) | yes |

`app` is one of `claude`, `claude-desktop`, `codex`, `gemini`, `grokbuild`,
`opencode`, `openclaw`, `hermes` (the same ids and aliases the GUI accepts).
OpenCode, OpenClaw and Hermes are additive apps with no single "current"
provider, so `ccs switch` refuses them.

### Global options

| Option | Meaning |
| --- | --- |
| `--json` | Print machine-readable JSON on stdout (errors become `{"error": ..., "code": ...}`) |
| `--config-dir DIR` | Use this data directory instead of the configured one |
| `-v`, `-vv` | Log info / debug messages to stderr |

### Exit codes

| Code | Meaning |
| --- | --- |
| 0 | success |
| 1 | the command failed (message on stderr) |
| 2 | invalid arguments |
| 3 | the database writer is held by another process (GUI, headless proxy or another `ccs`) |

### Examples

```bash
ccs ls claude
ccs switch claude packycode
ccs --json current codex | jq -r .provider.id
ccs usage summary --days 7 --app claude
ccs export ~/cc-switch-$(date +%F).sql
```

## Data directory

`ccs` resolves the data directory (`~/.cc-switch` by default) the same way the
GUI does, in this order:

1. `--config-dir DIR`
2. the `CC_SWITCH_CONFIG_DIR` environment variable
3. the custom directory chosen in the GUI (Settings → Directories), read from
   the GUI's store file `<app data dir>/com.ccswitch.desktop/app_paths.json`
   (`~/.local/share/...` on Linux, `~/Library/Application Support/...` on
   macOS, `%APPDATA%\...` on Windows)
4. `~/.cc-switch`

`ccs` refuses to touch a database whose schema is newer than it supports, and
leaves a not yet migrated legacy `config.json` for the GUI's interactive
migration.

## Running alongside the desktop app

SQLite runs in WAL mode, so reading while the app is open is always safe. A
provider switch, an import or a proxy takeover, however, is a multi-step
operation that also rewrites live config files and takeover backups, so only
one process may do it at a time. That is enforced with an OS advisory lock on
`<data dir>/cc-switch.lock` (holder details in `cc-switch.lock.json`):

- the desktop app takes the lock at launch and holds it until it exits
  (including while it sits in the tray);
- `ccs proxy start` / `cc-switch --headless` hold it while the proxy runs;
- `ccs switch` and `ccs import` hold it for the duration of the command.

When the lock is busy, write commands exit with code 3 and say who holds it,
for example:

```
ccs: The CC Switch database is in use by the CC Switch desktop app (pid 4242). Make the change in the desktop app (window or tray menu), or quit it and run the command again.
```

The OS releases the lock when the holder exits, even after a crash, so a
leftover lock file never blocks anyone.

Changes made with `ccs` while the app is closed show up the next time the app
starts. WebDAV / S3 auto-sync only runs inside the desktop app; changes made
with `ccs` are uploaded on the app's next sync.

## Headless proxy

```bash
ccs proxy start            # or: cc-switch --headless
ccs proxy start --no-takeover
ccs proxy status
ccs proxy stop
```

The headless proxy follows the GUI's lifecycle exactly:

- **Start:** takeover residue left by a crash is repaired first; then takeover
  is re-enabled for every app whose takeover switch is on in Settings → Proxy
  (Claude, Codex, Gemini, Grok Build), and the listener starts on the
  configured address and port. `--no-takeover` starts only the listener.
- **Stop** (Ctrl-C, SIGTERM, SIGHUP, or `ccs proxy stop`): live configs are
  restored to the direct provider settings while the per-app takeover switches
  are kept, so the next start takes over again. Queued usage logs are flushed.
- Daily maintenance (database backups, log pruning) runs as in the app.

`ccs proxy stop` asks the headless process to shut down through a request file
(`<data dir>/proxy-daemon.stop`) and waits for it to release the writer lock,
which works the same on every platform. It does not stop a proxy owned by the
desktop app; stop that one from Settings → Proxy.

`cc-switch --headless` accepts `--no-takeover`, `--config-dir DIR`, `--json`
(a one-line `started` event, then the final `stopped` result object) and
`-v`. It never creates a window or a tray icon and does not need a display.
On Windows release builds the desktop binary has no console attached, so use
`ccs proxy start` when you want to see its output.

Example systemd user unit:

```ini
# ~/.config/systemd/user/cc-switch-proxy.service
[Unit]
Description=CC Switch headless proxy

[Service]
ExecStart=%h/.local/bin/ccs proxy start
Restart=on-failure

[Install]
WantedBy=default.target
```

Do not run the headless proxy and the desktop app at the same time: they
would both try to bind the proxy port. The app prints a warning on stderr when
it starts while a headless proxy holds the lock.

## Implementation notes

- Entry points: `src-tauri/src/bin/ccs.rs` → `cc_switch_lib::cli::main()`;
  `src-tauri/src/main.rs` checks for `--headless` before starting Tauri.
- Module layout: `src-tauri/src/cli/` — `args.rs` (clap definitions),
  `config_dir.rs` (directory resolution), `lock.rs` (writer lock),
  `ops.rs` (commands), `headless.rs` (foreground proxy), `mod.rs` (dispatch,
  output, exit codes).
- Tests: unit tests in each module (argument parsing, lock semantics, config
  dir precedence) and `src-tauri/tests/cli.rs`, which drives `cli::run`
  against a temporary HOME.
