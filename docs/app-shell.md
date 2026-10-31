# App shell: IPC, events, startup and shutdown

Developer notes for the Tauri side of CC Switch: how the webview talks to the
backend, which events exist, what happens at startup and at exit, and the rules
commands follow. Code references are to `src-tauri/src/`.

## Layers

```
webview (React)  ──invoke──▶  commands/*.rs  ──▶  services/*  ──▶  database/dao/*, adapters/*
                 ◀──events──  (tray.rs, lib.rs, services emit)
```

- **Commands** (`commands/`) are thin: validate input, call a service, map the
  error. Business logic belongs in `services/`; per-app file formats belong in
  `adapters/`.
- **Typed IPC.** Commands marked `#[specta::specta]` and listed in
  `commands/bindings.rs` are exported to `src/lib/bindings.ts`
  (`pnpm gen:bindings`; the `bindings_are_up_to_date` test fails when stale).
  Frontend wrappers in `src/lib/api/` call `commands.*` for typed commands and
  `invoke("name", …)` for the rest. Every command must also be registered in
  `tauri::generate_handler!` in `lib.rs` and have an MSW handler in
  `tests/msw/handlers.ts` (`tests/contracts/commandRegistry.test.ts`).
- **App enumeration.** Iterate apps through `app_registry.rs`
  (`all_app_types()`, `takeover_app_types()`), not hand-written lists.

## Command rules

- Commands are `async`. Blocking work (file I/O, SQLite, process spawning,
  modal dialogs) runs on the blocking pool via `run_blocking` /
  `run_blocking_with_state` (`commands/blocking.rs`). A plain `fn` command runs
  on the UI thread and freezes the window.
- Never construct a second `AppState` in a command: it would carry its own
  `ProxyService` that does not see the running proxy (guarded by a test in
  `commands/sync_support.rs`). Use `run_blocking_with_state`.
- Validate `app_type` strings with `canonical_app_type` (or take `AppType`).
- Tray updates go through `tray::request_tray_rebuild(app, force)`: debounced,
  and it skips `set_menu` when the menu model did not change (an open macOS menu
  stays open).
- Settings: `save_settings` sends the whole object; `patch_settings` changes
  only the given keys. Both merge under the settings write lock and protect
  backend-owned fields (current-provider pointers, migrations, sync secrets).

## Events

Event names are constants in `events.rs`; emit with them, not string literals.

| Event | Payload | Emitted by |
|---|---|---|
| `provider-switched` | `{ appType, providerId, source?, proxyEnabled?, autoFailoverEnabled? }` | tray, failover, profile apply, `set_auto_failover_enabled` |
| `proxy-flags-changed` | same as above | tray provider / Auto clicks |
| `usage-cache-updated` | usage cache entry | usage queries |
| `deeplink-import` / `deeplink-error` | import request / `{ url, error }` | deep-link handler (buffered until the frontend calls `deeplink_frontend_ready`) |
| `profile-applied` | `{ profileId, scope }` | profile apply / clear |
| `universal-provider-synced` | none | startup import, universal provider sync |
| `update-download-progress` | `{ downloaded, total }` | `install_update_and_restart` |
| `proxy-restore-failed` | `{ appType, error }` | startup takeover restore, after one retry |
| `startup-progress` / `startup-ready` | phase timings | background startup (`init_status.rs`) |
| `usage-log-recorded` | none | `usage_events.rs` (debounced) |

Non-fatal startup problems are also kept for the UI to ask for:
`get_startup_warnings` returns `settings_corrupt`, `config_dir_unavailable` and
`proxy_restore_failed` entries. Fatal ones use `get_init_error`.

## Startup (`lib.rs::run`)

Critical path, before the window is shown:

1. panic hook; single-instance, deep-link and other plugins.
2. Data directory: the Store override is resolved **once**
   (`app_store::refresh_app_config_dir_override`). Changing the data directory in
   Settings only takes effect after a restart.
3. Logging: the previous run's log is kept as `logs/cc-switch.prev.log`;
   the live log rotates at 32 MB keeping two archives. The level is the default
   (`info`) until the stored `log_config` is read; noisy crates are capped at
   `warn`.
4. Database: version precheck (a too-new DB shows the recovery screen), schema
   migration with a retry/exit dialog, then the one-time `config.json` import.
   If that import fails the user gets the same retry/exit dialog; exiting
   removes the half-created database so the next launch retries, and
   `config.json` is only renamed to `.migrated` after a successful import.
5. Managed state, tray (skeleton menu), window setup (`main_window.rs`), and
   show the window unless silent startup is on.

Background (`run_background_startup`): live-config imports, sync workers, full
tray menu, crash recovery of takeover residue, proxy takeover restore (one
retry, then `proxy-restore-failed`), database maintenance, auto-launch
re-registration, then the periodic timers (daily maintenance on wall-clock time,
60 s session usage sync).

## Windows

All show/hide paths go through `main_window.rs`: `show_main_window` restores the
Windows taskbar button and the macOS Dock icon, `hide_main_window_to_tray`
removes them, and `apply_window_setup` applies the Linux WebKitGTK and
decoration setup to both the startup window and a window recreated by leaving
lightweight mode. Closing hides to the tray only when `minimizeToTrayOnClose`
is on **and** the tray icon is visible (`showInTray`); otherwise it exits.

## Shutdown

- **Quit** (tray "Quit", `app.exit(0)`): `RunEvent::ExitRequested` saves window
  state, runs `cleanup_before_exit` (stop the proxy and restore live files,
  bounded by an 8 s timeout so a stuck proxy cannot block exit), removes the
  tray icon, flushes logs and calls `std::process::exit(0)`.
- **Restart** (`app.restart()`, updater): handed to Tauri's default restart;
  no custom cleanup runs concurrently with plugin exit hooks (that deadlocked,
  see the comment in `lib.rs`). `restart_app` restores live files first.
- **Windows update install**: cleanup runs before `install()`, which exits the
  process itself.
