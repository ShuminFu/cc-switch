# CCS-182: [shell] Add a StartupStep pipeline with a startup diagnostics report and a single run-event router

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P5`, `p2-medium`, `area:shell`

> **Ticket key:** `p5-app-shell-bootstrap` · **Phase:** P5 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p5-thin-commands`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
lib.rs run() is a monolith. A failed migration or live import shows up as a silent "missing providers" (backlog: startup diagnostics).

## Proposed change
- app_shell/bootstrap.rs with StartupStep, plus a get_startup_report command and UI.
- run_events.rs, a single router that includes the deep-link handling.
- Move the lightweight-mode AtomicBool into AppState.

## Acceptance criteria / verification
- [x] lib.rs run() is 300 lines or fewer
- [x] Test: a failing step appears in the report with its error code
- [x] `cargo test --manifest-path src-tauri/Cargo.toml bootstrap`

## Dependencies
p5-thin-commands

## Implementation notes

**Summary**
- `src-tauri/src/app_shell/bootstrap.rs` (new): `StartupStep<C>` (stable id + `fn(&C) -> StepResult`), `StepOutcome` (`Done`/`Skipped`), `StepError { code: ErrorCode, message }` (`From<AppError>` goes through `IpcError`, so the codes match the IPC ones), and a `StartupReport` that times each step and records `{ id, status: ok|skipped|failed, code, message, durationMs }`. A failed step is logged and recorded, and the steps after it still run, so the old log-and-continue behaviour is kept. The steps that ran synchronously are published to a global report. Background steps (Codex history migrations, crash recovery of live takeover) append to it through `run_background_step`. The whole `setup` closure moved here, split into `init_logging`, `init_app_state` (steps `legacy_config_load`, `db_version_check` (`DB_VERSION_TOO_NEW` on the recovery path), `database_init`, `legacy_json_migration`), `DATA_IMPORT_STEPS` (skill repos, skills SSOT, secrets migration, `live_config_import`, official seed, OpenCode/OpenClaw/Hermes sync, OMO/OMO Slim, MCP, prompts), `app_config_dir_migration`, `build_tray`, `manage_runtime_services`, `spawn_post_setup_tasks` and `show_main_window`.
- `live_config_import` now treats "file missing / generic import unsupported" (`*.missing`, `*.import_unsupported`, `Unsupported`) as nothing to import. Any other error (for example a corrupt `settings.json`) fails the step with its code (such as `JSON`). Before, it was a `debug` log line and the provider list was silently empty.
- `src-tauri/src/app_shell/run_events.rs` (new): a single deep-link router (`route_deeplinks` → `handle_deeplink_url`, the only place deep-link URLs are logged with redaction) used by the single-instance args, `on_open_url` and macOS `RunEvent::Opened`. It also holds `on_second_instance`, `register_deep_links`, `on_window_event`, `on_run_event`, and `ExitRequestAction`/`classify_exit_request` with their tests.
- The lightweight-mode `AtomicBool` moved from a `static` in `lightweight.rs` into `AppState` (`is_lightweight_mode`/`set_lightweight_mode`). `lightweight::is_lightweight_mode(app)` reads it through `try_state`, so it returns false in DB-recovery mode, where no AppState is managed.
- `lib.rs`: `run()` is 37 lines (it was ~1460). The command list moved into `fn invoke_handler()`, still `tauri::generate_handler![...]`, so `bindings.rs` still parses it. `cleanup_before_exit`, the proxy restore helpers and the dialogs stay in `lib.rs` to keep the diff small for the P4 lane.
- New command `get_startup_report` (`commands/system.rs`). TS types (`StartupReport`, `StartupStepRecord`, `StartupStepStatus`) are generated in `src/shared/bindings.ts`.
- UI: `StartupReportPanel` under Settings → Advanced → Startup Diagnostics lists each step with its status, error code and message. `useAppShellEvents` shows a warning toast at startup when any step failed. i18n keys `settings.advanced.startupReport.*` are added in en/zh/zh-TW/ja.

**Files:** `src-tauri/src/app_shell/{mod,bootstrap,run_events}.rs`, `src-tauri/src/{lib,store,lightweight,tray,bindings}.rs`, `src-tauri/src/commands/system.rs`, `src/shared/bindings.ts`, `src/lib/api/settings.ts`, `src/components/settings/{StartupReportPanel,SettingsPage}.tsx`, `src/hooks/useAppShellEvents.ts`, `src/i18n/locales/*.json`, `tests/components/StartupReportPanel.test.tsx`, `tests/msw/handlers.ts`, `tests/i18n/usedKeys.test.ts`.

**Commits:** `e3ccc9c` (code), plus the follow-up commit with these notes.

**Verification**
- `cargo test --manifest-path src-tauri/Cargo.toml bootstrap`: 6 passed. Among them, `failing_step_is_reported_with_its_error_code_and_later_steps_still_run` covers the acceptance test. `corrupt_live_config_fails_the_import_step_with_its_error_code` is the regression test: before this change the corrupt live config was only a debug log.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: 3009 passed, 0 failed (lib: 2777 passed, 3 ignored).
- `UPDATE_BINDINGS=1 cargo test --lib bindings` regenerated the bindings, then `bindings_are_up_to_date` passes.
- `cargo fmt` is clean. `cargo clippy -- -D warnings` and `cargo clippy --all-targets --features testkit -- -D warnings` are clean.
- `pnpm typecheck` is clean. `pnpm test:unit`: 164 files / 1569 tests passed. `pnpm format:check` is clean.
- `lib.rs` `run()`: 37 lines.

**Not verified locally**
- The macOS-only code (`RunEvent::Reopen`/`Opened` in `run_events.rs`, `macos_tray_icon` in `build_tray`) and the Windows-only code (`set_windows_app_user_model_id`, skip-taskbar, Windows debug deep-link registration) were not compiled. `cargo check --target x86_64-apple-darwin` fails in `objc2-exception-helper` (no Apple C toolchain). The `x86_64-pc-windows-gnu` check ran out of disk space. Those paths only call items that are not cfg-gated. The macOS and Windows CI jobs should confirm they build.
- Merge note: `fn invoke_handler()` re-indents the command list by 4 spaces. If the other lane adds commands to `generate_handler!`, re-add them inside `invoke_handler()` (`git merge -Xignore-space-change` handles this).
